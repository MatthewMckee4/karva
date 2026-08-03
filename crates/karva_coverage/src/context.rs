//! Stable formatting for static and dynamic coverage contexts.

/// Context used for execution outside a concrete test lifecycle.
pub(crate) const SESSION_CONTEXT: &str = "session";

/// Internal context replaced once first-attempt fixture setup reveals the full test identity.
pub(crate) const PENDING_SETUP_CONTEXT: &str = "\0karva-pending-setup";

/// Escapes one context component before `|`-separated composition.
fn escape_component(component: &str) -> String {
    component.replace('\\', "\\\\").replace('|', "\\|")
}

/// Formats optional static context followed by dynamic context components.
pub(crate) fn compose_context(static_context: Option<&str>, dynamic: &[&str]) -> Option<String> {
    static_context
        .into_iter()
        .chain(dynamic.iter().copied())
        .map(escape_component)
        .reduce(|mut context, component| {
            context.push('|');
            context.push_str(&component);
            context
        })
}

/// Prefixes an already-formatted dynamic context with one static component.
pub(crate) fn prefix_context(static_context: &str, dynamic: &str) -> String {
    let mut context = escape_component(static_context);
    if !dynamic.is_empty() {
        context.push('|');
        context.push_str(dynamic);
    }
    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_components_escape_separators_and_backslashes() {
        assert_eq!(
            compose_context(Some(r"os=win\dows"), &["test|case", "run"]),
            Some(r"os=win\\dows|test\|case|run".to_owned())
        );
    }
}
