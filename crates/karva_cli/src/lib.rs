use std::{
    ffi::OsString,
    io::{self, BufWriter, Write},
    process::{ExitCode, Termination},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use colored::Colorize;
use crossbeam::channel as crossbeam_channel;
use karva_core::{
    diagnostic::MainDiagnosticWriter,
    runner::{RunDiagnostics, Runner},
};
use karva_project::{
    path::{SystemPath, SystemPathBuf, deduplicate_nested_paths},
    project::Project,
};
use notify::Watcher as _;

use crate::{
    args::{Args, Command, TestCommand},
    logging::setup_tracing,
};

mod args;
mod logging;
mod version;

#[must_use]
pub fn karva_main(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> ExitStatus {
    run(f).unwrap_or_else(|error| {
        use std::io::Write;

        let mut stderr = std::io::stderr().lock();

        writeln!(stderr, "{}", "Karva failed".red().bold()).ok();
        for cause in error.chain() {
            if let Some(ioerr) = cause.downcast_ref::<io::Error>() {
                if ioerr.kind() == io::ErrorKind::BrokenPipe {
                    return ExitStatus::Success;
                }
            }

            writeln!(stderr, "  {} {cause}", "Cause:".bold()).ok();
        }

        ExitStatus::Error
    })
}

fn run(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> anyhow::Result<ExitStatus> {
    let args = wild::args_os();

    let args = f(
        argfile::expand_args_from(args, argfile::parse_fromfile, argfile::PREFIX)
            .context("Failed to read CLI arguments from file")?,
    );

    let args = Args::parse_from(args);

    match args.command {
        Command::Test(test_args) => test(&test_args),
        Command::Version => version().map(|()| ExitStatus::Success),
    }
}

pub(crate) fn version() -> Result<()> {
    let mut stdout = BufWriter::new(io::stdout().lock());
    let version_info = crate::version::version();
    writeln!(stdout, "karva {}", &version_info)?;
    Ok(())
}

pub(crate) fn test(args: &TestCommand) -> Result<ExitStatus> {
    let verbosity = args.verbosity.level();
    let _guard = setup_tracing(verbosity);

    let cwd = {
        let cwd = std::env::current_dir().context("Failed to get the current working directory")?;
        SystemPathBuf::from_path_buf(cwd)
            .map_err(|path| {
                anyhow!(
                    "The current working directory `{}` contains non-Unicode characters. Karva only supports Unicode paths.",
                    path.display()
                )
            })?
    };

    let mut paths: Vec<String> = deduplicate_nested_paths(args.paths.iter())
        .map(|path| SystemPath::absolute(path, &cwd).as_str().to_string())
        .collect();

    if args.paths.is_empty() {
        tracing::debug!(
            "Could not resolve provided paths, trying to resolve current working directory"
        );
        paths.push(cwd.as_str().to_string());
    }

    let project = Project::new(cwd, paths, args.test_prefix.clone());

    let (main_loop, main_loop_cancellation_token) = MainLoop::new(project);

    let main_loop_cancellation_token = Arc::new(Mutex::new(Some(main_loop_cancellation_token)));
    let token_clone = Arc::clone(&main_loop_cancellation_token);

    ctrlc::set_handler(move || {
        let value = token_clone.lock().unwrap().take();
        if let Some(token) = value {
            token.stop();
        }
        std::process::exit(0);
    })?;

    let exit_status = if args.watch {
        main_loop.watch()?
    } else {
        main_loop.run()?
    };

    Ok(exit_status)
}

#[derive(Copy, Clone)]
pub enum ExitStatus {
    /// Checking was successful and there were no errors.
    Success = 0,

    /// Checking was successful but there were errors.
    Failure = 1,

    /// Checking failed.
    Error = 2,
}

impl Termination for ExitStatus {
    fn report(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

impl ExitStatus {
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        self as i32
    }
}

struct MainLoop {
    sender: crossbeam_channel::Sender<MainLoopMessage>,
    receiver: crossbeam_channel::Receiver<MainLoopMessage>,
    watcher: Option<notify::RecommendedWatcher>,
    project: Arc<Project>,
}

impl MainLoop {
    fn new(project: Project) -> (Self, MainLoopCancellationToken) {
        let (sender, receiver) = crossbeam_channel::bounded(10);

        (
            Self {
                sender: sender.clone(),
                receiver,
                watcher: None,
                project: Arc::new(project),
            },
            MainLoopCancellationToken { sender },
        )
    }

    fn watch(mut self) -> anyhow::Result<ExitStatus> {
        let startup_time = std::time::Instant::now();
        let sender = self.sender.clone();

        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if let Ok(event) = res {
                // Ignore events in the first 500ms after startup
                if startup_time.elapsed() > std::time::Duration::from_millis(500) {
                    // Only respond to Python file changes
                    let is_python_file = event.paths.iter().any(|path| {
                        path.extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext == "py")
                    });

                    if is_python_file {
                        match event.kind {
                            notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
                            | notify::EventKind::Create(_)
                            | notify::EventKind::Remove(_) => {
                                sender.send(MainLoopMessage::ApplyChanges).unwrap();
                            }
                            _ => {}
                        }
                    }
                }
            }
        })?;

        watcher.watch(
            self.project.cwd().as_ref().as_std_path(),
            notify::RecursiveMode::Recursive,
        )?;

        self.watcher = Some(watcher);
        self.sender.send(MainLoopMessage::TestWorkspace).unwrap();
        self.run()
    }

    fn run(self) -> anyhow::Result<ExitStatus> {
        let mut revision = 0u64;
        let mut debounce_id = 0u64;

        if self.watcher.is_none() {
            self.sender.send(MainLoopMessage::TestWorkspace).unwrap();
        }

        while let Ok(message) = self.receiver.recv() {
            match message {
                MainLoopMessage::TestWorkspace => {
                    let project = Arc::clone(&self.project);
                    let sender = self.sender.clone();
                    let current_revision = revision;

                    let writer = Box::new(BufWriter::new(io::stdout()));
                    let mut diagnostics = MainDiagnosticWriter::new(writer);
                    let mut runner = Runner::new(&project, &mut diagnostics);
                    let result = runner.run();

                    sender
                        .send(MainLoopMessage::TestsCompleted {
                            result,
                            revision: current_revision,
                        })
                        .unwrap();
                }

                MainLoopMessage::TestsCompleted {
                    result,
                    revision: check_revision,
                } => {
                    if check_revision == revision {
                        let mut stdout = BufWriter::new(io::stdout().lock());

                        if result.passed() {
                            writeln!(stdout, "{}", "All checks passed!".green().bold())?;
                        } else {
                            writeln!(stdout, "{}", "Checks failed!".red().bold())?;
                        }

                        if self.watcher.is_none() {
                            return Ok(if result.passed() {
                                ExitStatus::Success
                            } else {
                                ExitStatus::Failure
                            });
                        }
                    }
                }

                MainLoopMessage::ApplyChanges => {
                    debounce_id += 1;
                    let current_debounce_id = debounce_id;
                    let sender = self.sender.clone();

                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        sender
                            .send(MainLoopMessage::DebouncedTest {
                                debounce_id: current_debounce_id,
                            })
                            .unwrap();
                    });
                }

                MainLoopMessage::DebouncedTest {
                    debounce_id: msg_debounce_id,
                } => {
                    if msg_debounce_id == debounce_id {
                        revision += 1;
                        self.sender.send(MainLoopMessage::TestWorkspace).unwrap();
                    }
                }

                MainLoopMessage::Exit => {
                    return Ok(ExitStatus::Success);
                }
            }
        }

        Ok(ExitStatus::Success)
    }
}

#[derive(Debug)]
struct MainLoopCancellationToken {
    sender: crossbeam_channel::Sender<MainLoopMessage>,
}

impl MainLoopCancellationToken {
    fn stop(self) {
        self.sender.send(MainLoopMessage::Exit).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[derive(Debug)]
enum MainLoopMessage {
    TestWorkspace,
    TestsCompleted {
        result: RunDiagnostics,
        revision: u64,
    },
    ApplyChanges,
    DebouncedTest {
        debounce_id: u64,
    },
    Exit,
}
