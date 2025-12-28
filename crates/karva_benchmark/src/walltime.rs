use std::sync::Once;

use divan::Bencher;
use karva_cli::SubTestCommand;
use karva_core::testing::setup_module;
use karva_logging::{Printer, VerbosityLevel};
use karva_metadata::ProjectMetadata;
use karva_project::ProjectDatabase;
use karva_projects::{InstalledProject, RealWorldProject};
use karva_system::OsSystem;

static SETUP_MODULE_ONCE: Once = Once::new();

pub struct ProjectBenchmark<'a> {
    installed_project: InstalledProject<'a>,
}

impl<'a> ProjectBenchmark<'a> {
    pub fn new(project: RealWorldProject<'a>) -> Self {
        let installed_project = project.setup(false).expect("Failed to setup project");
        Self { installed_project }
    }

    fn project(&self) -> (ProjectDatabase, SubTestCommand) {
        let test_paths = self
            .installed_project
            .config()
            .paths
            .iter()
            .map(ToString::to_string)
            .collect();

        let root = self.installed_project.path();

        let system = OsSystem::new(root);

        let metadata = ProjectMetadata::discover(
            root.as_path(),
            &system,
            self.installed_project.config.python_version,
        )
        .unwrap();

        let args = SubTestCommand {
            paths: vec![test_paths],
            no_ignore: Some(true),
            output_format: Some(karva_cli::OutputFormat::Concise),
            no_progress: Some(true),
            ..SubTestCommand::default()
        };

        (ProjectDatabase::new(metadata, system).unwrap(), args)
    }
}

fn test_project(project: &ProjectDatabase, args: &SubTestCommand) {
    let printer = Printer::new(VerbosityLevel::Default, true);

    let num_workers = karva_system::max_parallelism().get();

    let config = karva_runner::ParallelTestConfig { num_workers };

    karva_runner::run_parallel_tests(project, &config, args, printer).unwrap();
}

pub fn bench_project(bencher: Bencher, benchmark: &ProjectBenchmark) {
    SETUP_MODULE_ONCE.call_once(|| {
        setup_module();
    });

    bencher
        .with_inputs(|| benchmark.project())
        .bench_local_refs(|(db, args)| test_project(db, args));
}

pub fn warmup_project(benchmark: &ProjectBenchmark) {
    SETUP_MODULE_ONCE.call_once(|| {
        setup_module();
    });

    let (project, args) = benchmark.project();

    test_project(&project, &args);
}
