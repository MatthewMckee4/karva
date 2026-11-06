//! Terminal and native JSON views over the same filtered result set.

use std::io::{self, Write};

use anyhow::Result;
use karva_mutation::{MutationRecord, MutationResults, Outcome};

use crate::ExitStatus;

pub(super) fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Pending => "pending",
        Outcome::Killed => "killed",
        Outcome::Survived => "survived",
        Outcome::Uncovered => "uncovered",
        Outcome::TimedOut => "timed-out",
        Outcome::Invalid => "invalid",
        Outcome::Errored => "errored",
    }
}

pub(super) fn exit_status(records: impl Iterator<Item = Outcome>) -> ExitStatus {
    let outcomes: Vec<_> = records.collect();
    if outcomes
        .iter()
        .any(|outcome| matches!(outcome, Outcome::Errored | Outcome::Pending))
    {
        ExitStatus::Error
    } else if outcomes.iter().any(|outcome| {
        matches!(
            outcome,
            Outcome::Survived | Outcome::Uncovered | Outcome::TimedOut
        )
    }) {
        ExitStatus::Failure
    } else {
        ExitStatus::Success
    }
}

pub(super) fn results(results: &MutationResults, json: bool) -> Result<ExitStatus> {
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut stdout, results)?;
        writeln!(stdout)?;
    } else {
        for record in results.records.values() {
            writeln!(
                stdout,
                "{} {} {}:{} {}",
                record.mutant.id,
                outcome_name(record.outcome),
                record.mutant.path,
                record.mutant.line,
                record.mutant.symbol
            )?;
            writeln!(stdout, "  {}", record.reason)?;
            if record.context_fingerprint != results.context_fingerprint {
                writeln!(
                    stdout,
                    "  stale: execution context changed; retest required"
                )?;
            }
            if matches!(record.outcome, Outcome::TimedOut | Outcome::Errored) {
                if let Some(diagnostic) = &record.diagnostic {
                    let concise = diagnostic
                        .split("; evidence:")
                        .next()
                        .unwrap_or(diagnostic)
                        .split("; test evidence:")
                        .next()
                        .unwrap_or(diagnostic);
                    writeln!(stdout, "  {concise}")?;
                }
            }
        }
        for outcome in [
            Outcome::Killed,
            Outcome::Survived,
            Outcome::Uncovered,
            Outcome::TimedOut,
            Outcome::Invalid,
            Outcome::Errored,
            Outcome::Pending,
        ] {
            let count = results
                .records
                .values()
                .filter(|record| record.outcome == outcome)
                .count();
            writeln!(stdout, "{}: {count}", outcome_name(outcome))?;
        }
    }
    if results
        .records
        .values()
        .any(|record| record.context_fingerprint != results.context_fingerprint)
    {
        Ok(ExitStatus::Error)
    } else {
        Ok(exit_status(
            results.records.values().map(|record| record.outcome),
        ))
    }
}

pub(super) fn show(record: &MutationRecord, original: &str, json: bool) -> Result<()> {
    let changed = record.mutant.apply(original)?;
    let diff = similar::TextDiff::from_lines(original, &changed)
        .unified_diff()
        .header(
            &format!("a/{}", record.mutant.path),
            &format!("b/{}", record.mutant.path),
        )
        .to_string();
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(
            &mut stdout,
            &serde_json::json!({"record": record, "diff": diff}),
        )?;
        writeln!(stdout)?;
    } else {
        writeln!(
            stdout,
            "{} {} {}:{}",
            record.mutant.id,
            outcome_name(record.outcome),
            record.mutant.path,
            record.mutant.line
        )?;
        writeln!(stdout, "duration: {:.6}s", record.duration_seconds)?;
        writeln!(stdout, "tests: {}", record.selected_tests.join(", "))?;
        writeln!(stdout, "{}", record.reason)?;
        if let Some(diagnostic) = &record.diagnostic {
            writeln!(stdout, "{diagnostic}")?;
        }
        write!(stdout, "{diff}")?;
    }
    Ok(())
}
