use std::sync::Once;

use divan::Bencher;
use karva_cli::SubTestCommand;
use karva_core::testing::setup_module;
use karva_logging::{Printer, VerbosityLevel};
use karva_metadata::{Options, ProjectMetadata, SrcOptions};
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

    fn project(&self) -> ProjectDatabase {
        let test_paths = self
            .installed_project
            .config()
            .paths
            .iter()
            .map(ToString::to_string)
            .collect();

        let root = self.installed_project.path();

        let system = OsSystem::new(root);

        let mut metadata = ProjectMetadata::discover(
            root.as_path(),
            &system,
            self.installed_project.config.python_version,
        )
        .unwrap();

        metadata.apply_options(Options {
            src: Some(SrcOptions {
                include: Some(test_paths),
                respect_ignore_files: Some(false),
            }),
            ..Options::default()
        });

        ProjectDatabase::new(metadata, system).unwrap()
    }
}

fn test_project(project: &ProjectDatabase) {
    let printer = Printer::new(VerbosityLevel::Default, false);

    let num_workers = karva_system::max_parallelism().get();

    let config = karva_runner::ParallelTestConfig { num_workers };

    karva_runner::run_parallel_tests(project, &config, &SubTestCommand::default(), printer)
        .unwrap();
}

pub fn bench_project(bencher: Bencher, benchmark: &ProjectBenchmark) {
    SETUP_MODULE_ONCE.call_once(|| {
        setup_module();
    });

    bencher
        .with_inputs(|| benchmark.project())
        .bench_local_refs(|db| test_project(db));
}

pub fn warmup_project(benchmark: &ProjectBenchmark) {
    SETUP_MODULE_ONCE.call_once(|| {
        setup_module();
    });

    let project = benchmark.project();

    test_project(&project);
}
