use std::fmt::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use colored::Colorize;
use crossbeam_channel::unbounded;
use notify_debouncer_mini::new_debouncer;
use notify_debouncer_mini::notify::RecursiveMode;

use karva_cli::SubTestCommand;
use karva_logging::Printer;
use karva_project::Project;
use karva_runner::ParallelTestConfig;

use crate::print_test_output;

fn run_and_print(
    project: &Project,
    config: &ParallelTestConfig,
    sub_command: &SubTestCommand,
    printer: Printer,
    durations: Option<usize>,
) {
    let start_time = Instant::now();
    match karva_runner::run_parallel_tests(project, config, sub_command) {
        Ok(result) => {
            if let Err(err) = print_test_output(
                printer,
                start_time,
                &result,
                sub_command.output_format.as_ref(),
                durations,
            ) {
                tracing::error!("Failed to print test output: {err}");
            }
        }
        Err(err) => {
            use std::io::Write as _;
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "{} {err}", "error:".red().bold());
        }
    }
}

fn print_watching_message(printer: Printer) -> Result<()> {
    let mut stdout = printer.stream_for_requested_summary().lock();
    writeln!(stdout)?;
    writeln!(
        stdout,
        "{}",
        "Watching for file changes... (Ctrl+C to stop)".dimmed()
    )?;
    Ok(())
}

pub(crate) fn run_watch_loop(
    project: &Project,
    config: &ParallelTestConfig,
    sub_command: &SubTestCommand,
    printer: Printer,
    durations: Option<usize>,
) -> Result<()> {
    run_and_print(project, config, sub_command, printer, durations);

    let (tx, file_rx) = unbounded::<Vec<PathBuf>>();
    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        move |res: notify_debouncer_mini::DebounceEventResult| {
            if let Ok(events) = res {
                let py_paths: Vec<_> = events
                    .into_iter()
                    .filter(|e| e.path.extension().is_some_and(|ext| ext == "py"))
                    .map(|e| e.path)
                    .collect();
                if !py_paths.is_empty() {
                    let _ = tx.send(py_paths);
                }
            }
        },
    )?;

    debouncer
        .watcher()
        .watch(project.cwd().as_std_path(), RecursiveMode::Recursive)?;

    let shutdown_rx = karva_runner::shutdown_receiver();

    print_watching_message(printer)?;

    loop {
        crossbeam_channel::select! {
            recv(shutdown_rx) -> _ => {
                break;
            }
            recv(file_rx) -> result => {
                let Ok(changed_paths) = result else {
                    break;
                };

                // Drain any additional queued events
                let mut all_paths = changed_paths;
                while let Ok(more_paths) = file_rx.try_recv() {
                    all_paths.extend(more_paths);
                }
                all_paths.sort();
                all_paths.dedup();

                // Clear screen
                {
                    use std::io::Write as _;
                    let _ = std::io::stdout().write_all(b"\x1B[2J\x1B[1;1H");
                }

                {
                    let mut stdout = printer.stream_for_requested_summary().lock();
                    writeln!(stdout, "{}", "File changes detected:".bold())?;
                    let cwd = project.cwd().as_std_path();
                    for path in &all_paths {
                        let display = path.strip_prefix(cwd).unwrap_or(path);
                        writeln!(stdout, "  {}", display.display().to_string().dimmed())?;
                    }
                    writeln!(stdout)?;
                }

                run_and_print(project, config, sub_command, printer, durations);

                print_watching_message(printer)?;
            }
        }
    }

    Ok(())
}
