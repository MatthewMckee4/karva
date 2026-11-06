//! Controlled repetitions distinguish observed instability from order dependence.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::Result;
use camino::Utf8Path;
use karva_collector::CollectedModule;
use karva_diagnostic::TestObservation;
use karva_project::Project;
use karva_runner::TestOrdering;
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
            let Some((module_name, name)) = function.rsplit_once("::") else {
                continue;
            };
            let Some(module) = modules
                .iter()
                .find(|module| module.path.module_name() == module_name)
            else {
                continue;
            };
            let Some(function) = module
                .test_function_defs
                .iter()
                .find(|function| function.name.as_str() == name)
            else {
                continue;
            };
            findings.push(Finding {
                rule, severity: "warning",
                location: Location::new(module.path.path(), function.name.range, modules, root),
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
