use std::sync::Once;

use camino::Utf8PathBuf;
use divan::Bencher;
use karva_core::{TestRunner, testing::setup_module};
use karva_project::{
    Options, OsSystem, ProjectDatabase, ProjectMetadata, RangedValue, SrcOptions, VerbosityLevel,
    absolute,
};
use karva_test::{InstalledProject, RealWorldProject};

pub struct ProjectBenchmark<'a> {
    installed_project: InstalledProject<'a>,
}

static SETUP_MODULE_ONCE: Once = Once::new();

impl<'a> ProjectBenchmark<'a> {
    pub fn new(project: RealWorldProject<'a>) -> Self {
        let installed_project = project.setup(false).expect("Failed to setup project");
        Self { installed_project }
    }

    fn project(&self) -> ProjectDatabase {
        let test_paths = self.installed_project.config().paths;

        let absolute_test_paths: Vec<Utf8PathBuf> = test_paths
            .iter()
            .map(|path| absolute(path, self.installed_project.path()))
            .collect();

        let root = self.installed_project.path();

        let system = OsSystem::new(root);

        let mut metadata = ProjectMetadata::discover(
            root.as_path(),
            &system,
            self.installed_project.config.python_version,
            VerbosityLevel::default(),
        )
        .unwrap();

        metadata.apply_options(Options {
            src: Some(SrcOptions {
                include: Some(RangedValue::cli(
                    absolute_test_paths
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                )),
                respect_ignore_files: Some(RangedValue::cli(false)),
            }),
            ..Options::default()
        });

        ProjectDatabase::new(metadata, system).unwrap()
    }
}

pub fn bench_project(bencher: Bencher, benchmark: &ProjectBenchmark) {
    fn test_project(project: &ProjectDatabase) {
        let result = project.test();

        assert!(result.stats().total() > 0, "{:#?}", result.diagnostics());
    }

    SETUP_MODULE_ONCE.call_once(|| {
        setup_module();
    });

    bencher
        .with_inputs(|| benchmark.project())
        .bench_local_refs(|db| test_project(db));
}

pub fn warmup_project(benchmark: &ProjectBenchmark) {
    fn test_project(project: &ProjectDatabase) {
        let result = project.test();

        assert!(result.stats().total() > 0, "{:#?}", result.diagnostics());
    }

    SETUP_MODULE_ONCE.call_once(|| {
        setup_module();
    });

    let project = benchmark.project();

    test_project(&project);
}
