use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use camino::Utf8PathBuf;
use karva_collector::{CollectedPackage, CollectionSettings, collect_file};
use ruff_python_ast::PythonVersion;

use super::super::collection::TestInfo;

pub(super) fn test_info(qualified_name: &str) -> TestInfo {
    test_info_with_duration(qualified_name, None)
}

pub(super) fn test_info_with_duration(
    qualified_name: &str,
    duration: Option<Duration>,
) -> TestInfo {
    let (function_root, case_index) = qualified_name
        .rsplit_once('[')
        .and_then(|(function_root, suffix)| {
            suffix
                .strip_suffix(']')
                .and_then(|index| index.parse::<usize>().ok())
                .map(|index| (function_root, Some(index)))
        })
        .unwrap_or((qualified_name, None));
    TestInfo {
        module_name: Arc::from("test_module"),
        qualified_name: qualified_name.to_string(),
        path: qualified_name.into(),
        duration,
        function_root: Arc::from(function_root),
        case_index,
    }
}

pub(super) fn collected_package(
    source: &str,
) -> (tempfile::TempDir, Utf8PathBuf, CollectedPackage) {
    let (temp_dir, mut test_paths, package) =
        collected_package_with_files([("test_sample.py", source)]);
    let test_path = test_paths
        .remove("test_sample.py")
        .expect("test path should exist");

    (temp_dir, test_path, package)
}

pub(super) fn collected_package_with_files<const N: usize>(
    files: [(&str, &str); N],
) -> (
    tempfile::TempDir,
    HashMap<String, Utf8PathBuf>,
    CollectedPackage,
) {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf())
        .expect("temp path should be UTF-8");
    let settings = CollectionSettings {
        python_version: PythonVersion::PY312,
        test_function_prefix: "test_",
        respect_ignore_files: true,
        collect_fixtures: false,
        collect_doctests: false,
    };
    let mut package = CollectedPackage::new(root);
    let mut test_paths = HashMap::new();

    for (name, source) in files {
        let test_path = package.path.join(name);
        std::fs::write(&test_path, source).expect("write test file");
        let module = collect_file(&test_path, &package.path, &settings, &[])
            .expect("collect test file")
            .expect("test file should collect");
        package.add_module(module);
        test_paths.insert(name.to_string(), test_path);
    }

    (temp_dir, test_paths, package)
}
