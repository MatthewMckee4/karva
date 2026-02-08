use divan::Bencher;
use karva_cli::SubTestCommand;
use karva_metadata::{Options, ProjectMetadata, SrcOptions, TestOptions};
use karva_project::Project;

use crate::real_world_projects::{InstalledProject, RealWorldProject};

pub struct ProjectBenchmark<'a> {
    installed_project: InstalledProject<'a>,
}

impl<'a> ProjectBenchmark<'a> {
    pub fn new(project: RealWorldProject<'a>) -> Self {
        let installed_project = project.setup(false).expect("Failed to setup project");
        Self { installed_project }
    }

    fn project(&self) -> Project {
        let test_paths = self
            .installed_project
            .config()
            .paths
            .iter()
            .map(ToString::to_string)
            .collect();

        let root = self.installed_project.path();

        let mut metadata =
            ProjectMetadata::discover(root.as_path(), self.installed_project.config.python_version)
                .unwrap();

        metadata.apply_options(Options {
            src: Some(SrcOptions {
                include: Some(test_paths),
                ..SrcOptions::default()
            }),
            test: Some(TestOptions {
                try_import_fixtures: Some(self.installed_project.config.try_import_fixtures),
                ..TestOptions::default()
            }),
            ..Options::default()
        });

        Project::from_metadata(metadata)
    }
}

fn test_project(project: &Project) {
    let num_workers = karva_static::max_parallelism().get();

    let config = karva_runner::ParallelTestConfig {
        num_workers,
        no_cache: false,
        create_ctrlc_handler: false,
    };

    let args = SubTestCommand {
        no_ignore: Some(true),
        output_format: Some(karva_cli::OutputFormat::Concise),
        no_progress: Some(true),
        ..SubTestCommand::default()
    };

    karva_runner::run_parallel_tests(project, &config, &args).unwrap();
}

pub fn bench_project(bencher: Bencher, benchmark: &ProjectBenchmark) {
    bencher
        .with_inputs(|| benchmark.project())
        .bench_local_refs(|db| test_project(db));
}

pub fn warmup_project(benchmark: &ProjectBenchmark) {
    let project = benchmark.project();

    test_project(&project);
}
