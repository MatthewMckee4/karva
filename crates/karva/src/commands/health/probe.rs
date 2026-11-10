//! Controlled repetitions distinguish observed instability from order dependence.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::Result;
use camino::Utf8Path;
use karva_collector::CollectedModule;
use karva_diagnostic::TestObservation;
use karva_project::Project;
use karva_runner::TestOrdering;
use ruff_text_size::{TextRange, TextSize};
use serde::Serialize;

use super::runtime::{execution_errors, run, successful};
use super::{Finding, Location};

/// Complete probe inputs and evidence; no historical inference is used.
#[derive(Serialize)]
pub(super) struct ProbeReport {
    pub(super) success: bool,
    cwd: String,
    repetitions_per_order: u32,
    pub(super) seeds: Vec<u64>,
    reproduction: Vec<String>,
    pub(super) runs: Vec<ProbeRun>,
}

/// One fresh interpreter generation, recorded in actual completion order.
#[derive(Serialize)]
pub(super) struct ProbeRun {
    ordering: &'static str,
    seed: Option<u64>,
    repetition: u32,
    observations: Vec<TestObservation>,
    pub(super) errors: Vec<String>,
}

pub(super) fn probe(
    project: &Project,
    profile: Option<&str>,
    repetitions: u32,
    mut seeds: Vec<u64>,
    mut reproduction: Vec<String>,
) -> Result<ProbeReport> {
    if seeds.is_empty() {
        seeds.push(karva_runner::generate_random_seed());
    }
    seeds.sort_unstable();
    seeds.dedup();
    reproduction.extend(["--probe".to_owned(), repetitions.to_string()]);
    for seed in &seeds {
        reproduction.extend(["--probe-seed".to_owned(), seed.to_string()]);
    }
    let mut success = true;
    let mut runs = Vec::new();
    'orders: for seed in std::iter::once(None).chain(seeds.iter().copied().map(Some)) {
        for repetition in 1..=repetitions {
            let output = run(
                project,
                profile.map(str::to_owned),
                false,
                seed.map_or(TestOrdering::Stable, TestOrdering::SeededShuffle),
            )?;
            let stopped = output.timed_out || output.cancelled;
            success &= successful(&output);
            runs.push(ProbeRun {
                ordering: if seed.is_some() {
                    "seeded-shuffle"
                } else {
                    "stable"
                },
                seed,
                repetition,
                errors: execution_errors(&output),
                observations: output.results.observations,
            });
            if stopped {
                break 'orders;
            }
        }
    }
    Ok(ProbeReport {
        success,
        cwd: project.cwd().as_str().replace('\\', "/"),
        repetitions_per_order: repetitions,
        seeds,
        reproduction,
        runs,
    })
}

impl ProbeReport {
    pub(super) fn findings(
        &self,
        modules: &[Arc<CollectedModule>],
        root: &Utf8Path,
    ) -> Vec<Finding> {
        let Some(first) = self.runs.first() else {
            return Vec::new();
        };
        let mut conditions = BTreeMap::new();
        for run in &self.runs {
            let by_key = run
                .observations
                .iter()
                .map(|observation| (&observation.key, observation))
                .collect::<HashMap<_, _>>();
            conditions
                .entry(run.seed)
                .or_insert_with(Vec::new)
                .push((run, by_key));
        }
        if conditions.len() != self.seeds.len() + 1
            || conditions.values().any(|runs| {
                !usize::try_from(self.repetitions_per_order).is_ok_and(|count| count == runs.len())
            })
        {
            return Vec::new();
        }
        let mut findings = Vec::new();
        for test in &first.observations {
            let mut groups = Vec::new();
            let mut evidence = Vec::new();
            let mut complete = true;
            for (seed, runs) in &conditions {
                let mut outcomes = BTreeSet::new();
                for (run, by_key) in runs {
                    let Some(observation) = by_key.get(&test.key) else {
                        complete = false;
                        break;
                    };
                    outcomes.extend(observation.outcomes.iter().map(String::as_str));
                    let condition = seed.map_or_else(
                        || "stable".to_owned(),
                        |seed| format!("seeded-shuffle seed {seed}"),
                    );
                    evidence.push(format!(
                        "{condition} repetition {}: {}",
                        run.repetition,
                        observation.outcomes.join(" -> ")
                    ));
                }
                groups.push(outcomes);
            }
            if !complete {
                continue;
            }
            let rule = if groups.iter().any(|outcomes| outcomes.len() > 1) {
                "unstable-test"
            } else if groups
                .iter()
                .flat_map(|group| group.iter().copied())
                .collect::<BTreeSet<_>>()
                .len()
                > 1
            {
                "order-dependent-test"
            } else {
                continue;
            };
            let function = test.key.test_function_name();
            let location = function
                .rsplit_once("::")
                .and_then(|(module_name, name)| {
                    let module = modules
                        .iter()
                        .find(|module| module.path.module_name() == module_name)?;
                    let range = module
                        .test_function_defs
                        .iter()
                        .rfind(|function| function.name.as_str() == name)
                        .map(|function| function.name.range)
                        .or_else(|| {
                            module
                                .doctests
                                .iter()
                                .find(|doctest| doctest.name == name)
                                .map(|doctest| doctest.range)
                        })
                        .unwrap_or_else(|| TextRange::empty(TextSize::new(0)));
                    Some(Location::new(module.path.path(), range, modules, root))
                })
                .unwrap_or_else(|| Location {
                    path: format!("<{function}>"),
                    line: 1,
                    column: 1,
                    start: 0,
                    end: 0,
                });
            findings.push(Finding {
                rule, severity: "warning",
                location,
                evidence: format!("{} has conflicting observed outcomes: {}", test.key, evidence.join("; ")),
                related: Vec::new(),
                action: if rule == "order-dependent-test" {
                    "Inspect shared state and fixture isolation; each observed order repeated consistently but different orders disagreed."
                } else {
                    "Inspect nondeterminism or external state; repeated observations under the same order disagreed."
                },
                reproduction: Some(self.reproduction.clone()),
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use karva_collector::{CollectionSettings, collect_source};
    use karva_python_semantic::TestCacheKey;
    use ruff_python_ast::PythonVersion;

    fn observed_report(key: &str) -> ProbeReport {
        let run = |seed: Option<u64>, repetition: u32, outcome: &str| ProbeRun {
            ordering: if seed.is_some() {
                "seeded-shuffle"
            } else {
                "stable"
            },
            seed,
            repetition,
            observations: vec![TestObservation {
                key: TestCacheKey::function_name(key),
                name: key.to_owned(),
                outcomes: vec![outcome.to_owned()],
            }],
            errors: Vec::new(),
        };
        ProbeReport {
            success: false,
            cwd: "/project".to_owned(),
            repetitions_per_order: 2,
            seeds: vec![2],
            reproduction: Vec::new(),
            runs: vec![
                run(None, 1, "passed"),
                run(None, 2, "passed"),
                run(Some(2), 1, "failed"),
                run(Some(2), 2, "failed"),
            ],
        }
    }

    #[test]
    fn missing_source_does_not_discard_observed_conflicts() {
        let findings =
            observed_report("dynamic::test_case").findings(&[], Utf8Path::new("/project"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "order-dependent-test");
        assert_eq!(findings[0].location.path, "<dynamic::test_case>");
    }

    #[test]
    fn probe_locations_use_the_surviving_definition() {
        let root = Utf8PathBuf::from("/project");
        let module = collect_source(
            &root.join("test_sample.py"),
            &root,
            "def test_case(): pass\ndef test_case(): pass\n".to_owned(),
            &CollectionSettings {
                python_version: PythonVersion::latest(),
                test_function_prefix: "test",
                respect_ignore_files: true,
                collect_fixtures: true,
                collect_doctests: false,
            },
            &[],
        )
        .expect("source module");
        let findings =
            observed_report("test_sample::test_case").findings(&[Arc::new(module)], &root);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].location.path, "test_sample.py");
        assert_eq!(findings[0].location.line, 2);
    }
}
