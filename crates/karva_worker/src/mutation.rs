//! Installs a worker-local loader override before any project imports.

use std::ffi::CString;

use anyhow::{Context, Result, bail};
use karva_ipc::MutationSource;
use pyo3::exceptions::PySyntaxError;
use pyo3::prelude::*;
use pyo3::types::PyModule;

/// Keeps the injection state available until the test run has finished.
pub struct InstalledMutation(Py<PyAny>);

impl InstalledMutation {
    /// Distinguishes an unimported candidate from one exercised by Python's loader.
    pub fn used(&self) -> Result<bool> {
        Python::attach(|py| self.0.bind(py).get_item("used")?.extract()).map_err(Into::into)
    }
}

/// Compilation failures are returned separately from infrastructure errors.
pub enum Installation {
    /// Compilation succeeded and all imports in this worker see the replacement.
    Installed(InstalledMutation),

    /// This candidate cannot compile on the actual worker interpreter.
    Invalid(String),
}

/// Verifies the original source before replacing its standard source-loader view.
pub fn install(mutation: &MutationSource) -> Result<Installation> {
    let original = std::fs::read(&mutation.path)
        .with_context(|| format!("mutant {}: cannot read `{}`", mutation.id, mutation.path))?;
    if karva_mutation::fingerprint(&[&original]) != mutation.original_fingerprint {
        bail!(
            "mutant {}: stale source `{}`; requested fingerprint {}, actual {}",
            mutation.id,
            mutation.path,
            mutation.original_fingerprint,
            karva_mutation::fingerprint(&[&original])
        );
    }
    let code = CString::new(include_str!("mutation.py"))?;
    Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &code,
            c"karva_mutation_loader.py",
            c"_karva_mutation_loader",
        )?;
        match module
            .getattr("install")?
            .call1((mutation.path.as_str(), &mutation.source))
        {
            Ok(state) => Ok(Installation::Installed(InstalledMutation(state.unbind()))),
            Err(error) if error.is_instance_of::<PySyntaxError>(py) => {
                Ok(Installation::Invalid(format!(
                    "mutant {} in `{}` cannot compile: {error}",
                    mutation.id, mutation.path
                )))
            }
            Err(error) => Err(error.into()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::types::PyDict;

    #[test]
    fn mutation_loader_bypasses_original_bytecode_without_writing_source() {
        Python::initialize();
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("mutation_loader_example.py");
        std::fs::write(&path, "value = True\n").expect("original source");
        // Windows canonicalization includes the verbatim path prefix, while Python's
        // import loader can use the ordinary spelling of the same file.
        let canonical = std::fs::canonicalize(&path).expect("canonical source path");
        let path = path.to_str().expect("UTF-8 path");
        Python::attach(|py| {
            let globals = PyDict::new(py);
            globals.set_item("filename", path).expect("filename");
            py.run(c"import importlib.util\nimport importlib.machinery\nloader = importlib.machinery.SourceFileLoader('_mutation_example', filename)\noriginal_data = importlib.machinery.SourceFileLoader.get_data\noriginal_code = importlib.machinery.SourceFileLoader.get_code\nassert loader.get_code('_mutation_example') is not None\n", Some(&globals), None).expect("populate original bytecode");
            let mutation = MutationSource {
                id: "example".into(),
                path: canonical.to_str().expect("UTF-8 canonical path").into(),
                original_fingerprint: karva_mutation::fingerprint(&[b"value = True\n"]),
                source: "value = False\n".into(),
            };
            let Installation::Installed(installed) = install(&mutation).expect("install") else {
                panic!("valid source must compile");
            };
            let result = py.run(c"namespace = {}\nexec(loader.get_code('_mutation_example'), namespace)\nassert namespace['value'] is False\nassert loader.get_data(filename) == b'value = False\\n'\n", Some(&globals), None);
            py.run(c"importlib.machinery.SourceFileLoader.get_data = original_data\nimportlib.machinery.SourceFileLoader.get_code = original_code\n", Some(&globals), None).expect("restore loader");
            result.expect("mutated source and code");
            assert!(installed.used().expect("used"));
            assert_eq!(
                std::fs::read_to_string(path).expect("original source"),
                "value = True\n"
            );
            let invalid = MutationSource {
                source: "def broken(:".into(),
                ..mutation.clone()
            };
            assert!(matches!(
                install(&invalid).expect("invalid compilation"),
                Installation::Invalid(_)
            ));
            let stale = MutationSource {
                original_fingerprint: "stale".into(),
                ..mutation
            };
            assert!(install(&stale).is_err());
        });
    }
}
