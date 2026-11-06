//! User-facing mutation workflow over native discovery and existing worker orchestration.

mod execution;
mod files;
mod filters;
mod report;

use std::collections::BTreeSet;
use std::io::Write;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use karva_cli::{MutationAction, MutationCommand, MutationRunArgs};
use karva_logging::Printer;
use karva_metadata::{MutationOptions, Options, ProjectMetadata, ProjectOptionsOverrides};
use karva_mutation::{DiscoveryFilters, MutationRecord, MutationResults, Outcome};
use karva_project::Project;
use karva_project::path::absolute;
use karva_python_semantic::current_python_version;

use crate::ExitStatus;
use crate::utils::cwd;

/// Resolves normal profiles before performing any mutation operation.
pub fn mutation(args: MutationCommand) -> Result<ExitStatus> {
    let cwd = cwd()?;
    let config_file = args.config_file.as_ref().map(|path| absolute(path, &cwd));
    let version = current_python_version();
    let mut metadata = if let Some(config) = &config_file {
        ProjectMetadata::from_config_file(config, &cwd, version)?
    } else {
        ProjectMetadata::discover(&cwd, version)?
    };
    metadata.apply_overrides(
        &ProjectOptionsOverrides::new(config_file, Options::default())
            .with_profile(args.profile.clone()),
    )?;
    let project = Project::from_metadata(metadata);
    let path = project.cwd().join(".karva/mutation/results.json");
    match args.action {
        MutationAction::Results(arguments) => {
            let filter = filters::Filters::new(&arguments)?;
            let mut results = MutationResults::read(&path)?;
            results.records.retain(|_, record| filter.matches(record));
            report::results(&results, args.json)
        }
        MutationAction::Show { mutant } => {
            let results = MutationResults::read(&path)?;
            let id = filters::resolve_id(results.records.values(), &mutant)?;
            let record = results
                .records
                .get(&id)
                .context("resolved mutant missing")?;
            let source_path =
                files::canonical_source(project.cwd(), &project.cwd().join(&record.mutant.path))?;
            report::show(record, &std::fs::read_to_string(source_path)?, args.json)?;
            Ok(ExitStatus::Success)
        }
        MutationAction::Apply { mutant } => {
            let _lock = files::lock(project.cwd())?;
            let results = MutationResults::read(&path)?;
            let id = filters::resolve_id(results.records.values(), &mutant)?;
            let record = results
                .records
                .get(&id)
                .context("resolved mutant missing")?;
            files::apply(project.cwd(), &record.mutant)?;
            if args.json {
                writeln!(
                    std::io::stdout(),
                    "{}",
                    serde_json::json!({"applied": id, "path": record.mutant.path})
                )?;
            } else {
                writeln!(std::io::stdout(), "Applied {id} to {}", record.mutant.path)?;
            }
            Ok(ExitStatus::Success)
        }
        MutationAction::Run(run) => run_mutations(
            &project,
            &run,
            args.profile,
            args.json,
            CandidateSelection::Discover,
        ),
        MutationAction::Retest(mut retest) => {
            if !retest.run.source.is_empty() || !retest.run.exclude.is_empty() {
                bail!(
                    "`--source` and `--exclude` apply only to `mutate run`; use `--path` to select recorded candidates for retesting"
                );
            }
            retest.run.no_cache = true;
            run_mutations(
                &project,
                &retest.run,
                args.profile,
                args.json,
                CandidateSelection::Retest(retest.mutant.as_deref()),
            )
        }
    }
}

fn resolved_options(
    project: &Project,
    args: &MutationRunArgs,
) -> Result<(MutationOptions, usize, Option<f64>)> {
    let mut options = project
        .metadata()
        .options
        .mutation
        .clone()
        .unwrap_or_default();
    if !args.source.is_empty() {
        options.sources = Some(args.source.clone());
    }
    if !args.exclude.is_empty() {
        options.exclude = Some(args.exclude.clone());
    }
    if !args.filters.operator.is_empty() {
        options.operators = Some(args.filters.operator.clone());
    }
    let workers = args
        .workers
        .map(std::num::NonZeroUsize::get)
        .or(options.workers)
        .map_or_else(
            || karva_static::max_parallelism().map(std::num::NonZeroUsize::get),
            Ok,
        )?;
    if workers == 0 {
        bail!("mutation workers budget requested 0; allowed values are positive integers");
    }
    let timeout = args
        .timeout
        .or_else(|| options.timeout.map(|value| value.0));
    if let Some(timeout) = timeout {
        if !timeout.is_finite()
            || timeout <= 0.0
            || std::time::Duration::try_from_secs_f64(timeout)
                .ok()
                .is_none_or(|duration| {
                    duration.is_zero() || Instant::now().checked_add(duration).is_none()
                })
        {
            bail!(
                "mutation timeout budget requested {timeout}; allowed values are positive finite seconds representable on this platform"
            );
        }
    }
    Ok((options, workers, timeout))
}

/// Chooses new source discovery or a subset of recorded source-bound candidates.
#[derive(Clone, Copy)]
enum CandidateSelection<'a> {
    Discover,
    Retest(Option<&'a str>),
}

fn run_mutations(
    project: &Project,
    args: &MutationRunArgs,
    profile: Option<String>,
    json: bool,
    selection: CandidateSelection<'_>,
) -> Result<ExitStatus> {
    let filter = filters::Filters::new(&args.filters)?;
    let (options, workers, explicit_timeout) = resolved_options(project, args)?;
    let discovery_filters = DiscoveryFilters::new(
        options.operators.as_deref().unwrap_or_default(),
        &[],
        options.exclude_symbols.as_deref().unwrap_or_default(),
        options.exclude_lines.as_deref().unwrap_or_default(),
    )?;
    let exclusions = filters::globs(options.exclude.as_deref().unwrap_or_default())?;
    let _lock = files::lock(project.cwd())?;
    let data_path = project.cwd().join(".karva/mutation/results.json");
    let mut previous = if data_path.exists() {
        Some(MutationResults::read(&data_path)?)
    } else {
        None
    };
    let sources = options.sources.clone().unwrap_or_else(|| {
        vec![if project.cwd().join("src").is_dir() {
            "src".into()
        } else {
            ".".into()
        }]
    });
    let mut candidates = Vec::new();
    let mut discovered = BTreeSet::new();
    if matches!(selection, CandidateSelection::Retest(None)) {
        if let Some(results) = &mut previous {
            files::prune_stale(project.cwd(), results)?;
            results.write(&data_path)?;
        }
    }
    if let CandidateSelection::Retest(prefix) = selection {
        let results = previous
            .as_ref()
            .context("no mutation results to retest; run `uv run karva mutate run` first")?;
        let exact_id = prefix
            .map(|prefix| filters::resolve_id(results.records.values(), prefix))
            .transpose()?;
        for record in results.records.values() {
            if exact_id.as_ref().is_none_or(|id| id == &record.mutant.id) && filter.matches(record)
            {
                let source = std::fs::read_to_string(project.cwd().join(&record.mutant.path))?;
                record.mutant.apply(&source)?;
                candidates.push(record.mutant.clone());
            }
        }
    } else {
        for path in files::files(project.cwd(), &sources, true)? {
            let relative = files::relative(project.cwd(), &path)?;
            let test_file = relative.file_name().is_some_and(|name| {
                name.starts_with("test_")
                    || name.ends_with("_test.py")
                    || name == "conftest.py"
                    || name == "karva.py"
            });
            if path.extension() != Some("py")
                || test_file
                || relative
                    .components()
                    .any(|component| component.as_str() == "tests")
                || exclusions.is_match(&relative)
            {
                continue;
            }
            let source = std::fs::read_to_string(&path)?;
            for mutant in karva_mutation::discover(
                &relative,
                &source,
                current_python_version(),
                &discovery_filters,
            )? {
                discovered.insert(mutant.id.clone());
                let pending = MutationRecord {
                    mutant: mutant.clone(),
                    context_fingerprint: String::new(),
                    outcome: Outcome::Pending,
                    selected_tests: Vec::new(),
                    duration_seconds: 0.0,
                    reason: String::new(),
                    diagnostic: None,
                };
                let prior = previous
                    .as_ref()
                    .and_then(|results| results.records.get(&mutant.id))
                    .unwrap_or(&pending);
                if filter.matches(prior) {
                    candidates.push(mutant);
                }
            }
        }
    }
    if matches!(selection, CandidateSelection::Discover) {
        let roots = sources
            .iter()
            .map(|source| {
                let path = files::canonical_source(project.cwd(), &project.cwd().join(source))?;
                files::relative(project.cwd(), &path)
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some(results) = &mut previous {
            results.records.retain(|id, record| {
                !roots
                    .iter()
                    .any(|root| record.mutant.path.starts_with(root))
                    || discovered.contains(id)
            });
            results.write(&data_path)?;
        }
    }
    candidates.sort_by(|left, right| {
        (&left.path, left.start, left.end, &left.id).cmp(&(
            &right.path,
            right.start,
            right.end,
            &right.id,
        ))
    });
    let selected: BTreeSet<_> = candidates.iter().map(|mutant| mutant.id.clone()).collect();
    if selected.is_empty() {
        bail!(
            "no mutations match the supplied sources and filters; check mutation.sources, --source, --path, --symbol, --operator, and --outcome"
        );
    }
    let start = Instant::now();
    let baseline = execution::baseline(project, &sources, profile.clone())?;
    if baseline.output.cancelled {
        if json {
            writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"error": "mutation baseline cancelled"})
            )?;
        }
        bail!("mutation baseline cancelled; no mutants were executed");
    }
    if baseline.output.timed_out
        || !baseline.output.results.is_success()
        || baseline.output.results.stats.total() == 0
    {
        if json {
            writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"error": "mutation baseline failed", "baseline": baseline.output.results})
            )?;
        } else {
            super::test::print_test_output(
                Printer::default(),
                start,
                &baseline.output.results,
                None,
            )?;
        }
        bail!("mutation baseline failed; fix the unmodified test suite before running mutants");
    }
    // Receipt: CPython 3.13 on macOS, one passing test, three warm sequential
    // invocations measured 0.621–0.993 s. Eight invocations at concurrency four
    // measured 1.404–2.132 s (maximum ratio 3.432). A tenfold baseline budget
    // leaves headroom for process startup, instrumentation, and scheduling jitter.
    let budget_seconds = explicit_timeout.unwrap_or(baseline.elapsed_seconds * 10.0);
    let configuration = serde_json::json!({"sources": sources, "mutation": options, "workers": workers, "timeout_seconds": explicit_timeout, "profile": profile});
    let context = files::context(project, &configuration)?;
    let mut results = previous
        .take()
        .unwrap_or_else(|| MutationResults::new(context.clone(), configuration.clone()));
    results.context_fingerprint.clone_from(&context);
    results.configuration = configuration;
    results.baseline_seconds = baseline.elapsed_seconds;
    let mut pending = Vec::new();
    for mutant in candidates {
        if !args.no_cache {
            if let Some(record) = results.records.get_mut(&mutant.id) {
                if record.context_fingerprint == context
                    && !matches!(record.outcome, Outcome::Pending | Outcome::Errored)
                {
                    record.reason = "reused: matching source, tests, dependencies, runtime, environment, and configuration".into();
                    continue;
                }
            }
        }
        results.records.insert(
            mutant.id.clone(),
            MutationRecord {
                mutant: mutant.clone(),
                context_fingerprint: context.clone(),
                outcome: Outcome::Pending,
                selected_tests: Vec::new(),
                duration_seconds: 0.0,
                reason: "pending execution".into(),
                diagnostic: None,
            },
        );
        pending.push(mutant);
    }
    results.write(&data_path)?;
    let cancelled = execution::run(
        project,
        &baseline,
        &mut results,
        pending,
        execution::RunSettings {
            workers,
            budget_seconds,
            profile,
            full_suite: args.full_suite,
            json,
        },
    )?;
    results.records.retain(|id, _| selected.contains(id));
    let status = report::results(&results, json)?;
    if cancelled {
        Ok(ExitStatus::Error)
    } else {
        Ok(status)
    }
}
