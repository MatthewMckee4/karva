//! Test identity resolution before and after fixture setup.

use karva_python_semantic::QualifiedTestName;

use crate::utils::try_render_builtin_test_parameters;

use super::VariantRunner;

impl VariantRunner<'_, '_, '_, '_, '_> {
    /// Returns unresolved identity used for filtering and resume checks.
    pub(super) fn unresolved_test_name(&self) -> QualifiedTestName {
        if let Some(id) = &self.input.identity.id {
            QualifiedTestName::with_parameters(self.input.test.name().clone(), id.clone())
        } else {
            QualifiedTestName::new(self.input.test.name().clone())
        }
        .with_case_index(self.input.identity.case_index)
    }

    /// Returns exact identity before fixture setup when possible.
    ///
    /// Initial checkpoint begins setup, not test body. Fixture-derived
    /// parameters remain unresolved until setup completes so setup crashes
    /// still have flushed active-test checkpoint.
    pub(super) fn initial_test_name(
        &self,
        unresolved: QualifiedTestName,
    ) -> (QualifiedTestName, bool) {
        if self.input.identity.id.is_some() {
            return (unresolved, true);
        }
        if !self.input.fixtures.dependencies.is_empty()
            || !self.input.fixtures.use_dependencies.is_empty()
            || !self.input.fixtures.auto_use.is_empty()
        {
            return (unresolved, false);
        }
        if self.input.params.is_empty() {
            return (unresolved, true);
        }

        let Some(parameters) = try_render_builtin_test_parameters(
            self.py,
            self.input
                .params
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_ref())),
            self.input.test.parameters(),
        ) else {
            return (unresolved, false);
        };

        (unresolved.with_resolved_parameters(parameters), true)
    }
}
