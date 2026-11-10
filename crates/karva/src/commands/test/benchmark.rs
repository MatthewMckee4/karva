//! Human-readable benchmark summaries; structured reports retain raw measurements.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

use anyhow::Result;
use karva_diagnostic::{AggregatedResults, BenchmarkResult};

/// Groups measurements without hiding test parameters or incomplete sampling.
pub(super) fn write_benchmarks(
    stdout: &mut impl Write,
    results: &AggregatedResults,
    verbose: bool,
) -> Result<()> {
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for case in &results.test_cases {
        for benchmark in case.benchmarks() {
            groups
                .entry(benchmark.group.as_deref())
                .or_default()
                .push((case.full_name(), benchmark));
        }
    }
    if groups.is_empty() {
        return Ok(());
    }

    writeln!(stdout, "\nbenchmarks (per call):")?;
    for (group, mut benchmarks) in groups {
        benchmarks.sort_by_key(|(name, _)| *name);
        writeln!(stdout, "\n  {}", group.unwrap_or("ungrouped"))?;
        let mut rows = vec![
            [
                "Test", "Min", "Median", "Mean", "Stddev", "Rounds", "Iters", "Status",
            ]
            .map(str::to_owned),
        ];
        for (name, benchmark) in &benchmarks {
            rows.push([
                (*name).to_owned(),
                format_time(benchmark.min_ns),
                format_time(benchmark.median_ns),
                format_time(benchmark.mean_ns),
                format_time(benchmark.stddev_ns),
                benchmark.rounds.to_string(),
                benchmark.iterations.to_string(),
                match benchmark.stop_reason.as_str() {
                    "disabled" => "disabled",
                    "exception" => "partial",
                    "rounds" | "max_time" => "complete",
                    reason => reason,
                }
                .to_owned(),
            ]);
        }
        let widths = rows.iter().fold([0; 8], |mut widths, row| {
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.chars().count());
            }
            widths
        });
        for row in rows {
            write!(stdout, "    {:<width$}", row[0], width = widths[0])?;
            for (cell, width) in row.iter().zip(widths).skip(1) {
                write!(stdout, "  {cell:>width$}")?;
            }
            writeln!(stdout)?;
        }
        if verbose {
            for (name, benchmark) in benchmarks {
                write_details(stdout, name, benchmark)?;
            }
        }
    }
    writeln!(stdout)?;
    Ok(())
}

/// Uses standard duration units while retaining integer nanoseconds below one µs.
fn format_time(nanoseconds: Option<u64>) -> String {
    match nanoseconds {
        None => "-".to_owned(),
        Some(ns @ 0..=999) => format!("{ns}ns"),
        Some(ns) => format!("{:.2?}", Duration::from_nanos(ns)),
    }
}

fn write_details(stdout: &mut impl Write, name: &str, benchmark: &BenchmarkResult) -> Result<()> {
    writeln!(stdout, "\n    {name}")?;
    writeln!(
        stdout,
        "      warmup={} calibration={} measured={} total={} elapsed={} stop={}",
        benchmark.warmup_calls,
        benchmark.calibration_calls,
        benchmark.measurement_calls,
        benchmark.total_calls,
        format_time(Some(benchmark.elapsed_ns)),
        benchmark.stop_reason
    )?;
    writeln!(
        stdout,
        "      options: {}",
        serde_json::to_string(&benchmark.options)?
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::format_time;

    #[rstest]
    #[case(None, "-")]
    #[case(Some(0), "0ns")]
    #[case(Some(999), "999ns")]
    #[case(Some(1_500), "1.50µs")]
    #[case(Some(1_500_000), "1.50ms")]
    #[case(Some(1_500_000_000), "1.50s")]
    fn measurement_units(#[case] nanoseconds: Option<u64>, #[case] expected: &str) {
        assert_eq!(format_time(nanoseconds), expected);
    }
}
