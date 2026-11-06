//! Extracts exception groups before Python references leave the worker.
//!
//! Measurement receipt: a 64-leaf, eight-level group with one frame and note per
//! leaf formatted to 24,995 bytes with Python's traceback formatter. A
//! recursive failure at depth 64 retained 66 frames. The longest note was 41 bytes. The limits below allow
//! substantially larger reports while bounding branching, chains and text jointly.

use std::collections::VecDeque;

use karva_diagnostic::{ExceptionDiagnostic, ExceptionFrame};
use pyo3::exceptions::PyBaseExceptionGroup;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

const WIDTH: usize = 256;
const DEPTH: usize = 32;
const FRAMES: usize = 128;
const TOTAL_FRAMES: usize = 1024;
const TOTAL_NOTES: usize = 1024;
const NODES: usize = 512;
const VALUE_BYTES: usize = 16 * 1024;
const TEXT_BYTES: usize = 256 * 1024;

/// Unchained ordinary failures bypass attribute lookup and tree allocation.
pub(super) fn extract(py: Python<'_>, error: &PyErr) -> Option<ExceptionDiagnostic> {
    if !error.is_instance_of::<PyBaseExceptionGroup>(py)
        && error.cause(py).is_none()
        && error.context(py).is_none()
    {
        return None;
    }
    let group_type = py.get_type::<PyBaseExceptionGroup>();
    let group_type = group_type.as_any();
    let value = error.value(py).as_any();
    if !contains_group(value, group_type, &mut Vec::new()) {
        return None;
    }
    // On Python 3.11 PyO3 retains the root traceback separately from its value.
    let value = error.clone_ref(py).into_value(py);
    let value = value.bind(py).as_any();
    Budget {
        nodes: NODES,
        bytes: TEXT_BYTES,
        frames: TOTAL_FRAMES,
        notes: TOTAL_NOTES,
        ancestors: Vec::new(),
    }
    .extract(value, group_type, 0)
}

/// Looks only along the visible chain; group children are inspected during extraction.
fn contains_group(
    value: &Bound<'_, PyAny>,
    group_type: &Bound<'_, PyAny>,
    seen: &mut Vec<usize>,
) -> bool {
    if value.is_instance(group_type).unwrap_or(false) {
        return true;
    }
    let identity = value.as_ptr() as usize;
    if seen.len() >= DEPTH || seen.contains(&identity) {
        return false;
    }
    seen.push(identity);
    visible_chain(value).is_some_and(|(_, chain)| contains_group(&chain, group_type, seen))
}

fn visible_chain<'py>(value: &Bound<'py, PyAny>) -> Option<(bool, Bound<'py, PyAny>)> {
    if let Ok(cause) = value.getattr("__cause__")
        && !cause.is_none()
    {
        return Some((true, cause));
    }
    if !value
        .getattr("__suppress_context__")
        .ok()?
        .is_truthy()
        .ok()?
        && let Ok(context) = value.getattr("__context__")
        && !context.is_none()
    {
        return Some((false, context));
    }
    None
}

/// All branches and chains consume the same budget; repeated siblings remain distinct.
struct Budget {
    nodes: usize,
    bytes: usize,
    frames: usize,
    notes: usize,
    ancestors: Vec<usize>,
}

impl Budget {
    fn text(&mut self, value: &Bound<'_, PyAny>, omissions: &mut Vec<String>) -> String {
        let Ok(value) = value.str() else {
            return "<exception text unavailable>".to_string();
        };
        let value = value.to_string_lossy();
        let requested = value.len();
        let mut end = requested.min(self.bytes).min(VALUE_BYTES);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        self.bytes -= end;
        if end < requested {
            omissions.push(format!(
                "exception text budget: limit {TEXT_BYTES} total bytes and {VALUE_BYTES} bytes per value, requested {requested} bytes with {} remaining; {} bytes hidden",
                self.bytes + end, requested - end,
            ));
        }
        value[..end].to_string()
    }

    fn extract(
        &mut self,
        value: &Bound<'_, PyAny>,
        group_type: &Bound<'_, PyAny>,
        depth: usize,
    ) -> Option<ExceptionDiagnostic> {
        if self.nodes == 0 || depth >= DEPTH || self.bytes == 0 {
            return None;
        }
        self.nodes -= 1;
        let identity = value.as_ptr() as usize;
        let mut result = ExceptionDiagnostic {
            exception_type: String::new(),
            message: String::new(),
            notes: Vec::new(),
            traceback: Vec::new(),
            children: Vec::new(),
            omitted_children: 0,
            omitted_frames: 0,
            cause: None,
            context: None,
            omissions: Vec::new(),
        };
        if let Ok(name) = value.get_type().getattr("__qualname__") {
            result.exception_type = self.text(&name, &mut result.omissions);
        }
        let is_group = value.is_instance(group_type).unwrap_or(false);
        let message = if is_group {
            value.getattr("message").ok()
        } else {
            None
        };
        result.message = self.text(message.as_ref().unwrap_or(value), &mut result.omissions);
        if self.ancestors.contains(&identity) {
            result
                .omissions
                .push("exception chain cycle: repeated exception hidden".to_string());
            return Some(result);
        }
        self.ancestors.push(identity);
        self.traceback(value, &mut result);
        if let Ok(notes) = value.getattr("__notes__")
            && let Ok(notes) = notes.cast::<PyList>()
        {
            let retained = notes.len().min(WIDTH).min(self.notes);
            for note in notes.iter().take(retained) {
                if self.bytes == 0 {
                    break;
                }
                result.notes.push(self.text(&note, &mut result.omissions));
                self.notes -= 1;
            }
            if result.notes.len() < notes.len() {
                result.omissions.push(format!(
                    "exception notes budget: limit {WIDTH} notes per exception, {TOTAL_NOTES} total and {TEXT_BYTES} text bytes, requested {} notes; {} notes hidden",
                    notes.len(), notes.len() - result.notes.len(),
                ));
            }
        }
        // Children take precedence over chains when the shared node budget is exhausted.
        if is_group
            && let Ok(children) = value.getattr("exceptions")
            && let Ok(children) = children.cast::<PyTuple>()
        {
            for child in children.iter().take(WIDTH) {
                let Some(child) = self.extract(&child, group_type, depth + 1) else {
                    break;
                };
                result.children.push(child);
            }
            result.omitted_children = children.len() - result.children.len();
            if result.omitted_children > 0 {
                result.omissions.push(format!(
                    "exception group budgets: width limit {WIDTH}, depth limit {DEPTH}, node limit {NODES}, text limit {TEXT_BYTES} bytes; requested {} children at depth {} with {} nodes and {} text bytes remaining",
                    children.len(), depth + 1, self.nodes, self.bytes,
                ));
            }
        }
        if let Some((explicit, chain)) = visible_chain(value) {
            if let Some(chain) = self.extract(&chain, group_type, depth + 1) {
                if explicit {
                    result.cause = Some(Box::new(chain));
                } else {
                    result.context = Some(Box::new(chain));
                }
            } else {
                result.omissions.push(format!(
                    "exception chain hidden: depth limit {DEPTH}, node limit {NODES}, text limit {TEXT_BYTES} bytes; requested depth {} with {} nodes and {} text bytes remaining",
                    depth + 1, self.nodes, self.bytes,
                ));
            }
        }
        self.ancestors.pop();
        Some(result)
    }

    /// Keeps the innermost frames without loading entire source files into reports.
    fn traceback(&mut self, value: &Bound<'_, PyAny>, result: &mut ExceptionDiagnostic) {
        let Ok(mut traceback) = value.getattr("__traceback__") else {
            return;
        };
        let limit = FRAMES.min(self.frames);
        let mut retained = VecDeque::new();
        let mut requested = 0;
        while !traceback.is_none() {
            requested += 1;
            retained.push_back(traceback.clone());
            if retained.len() > limit {
                retained.pop_front();
            }
            let Ok(next) = traceback.getattr("tb_next") else {
                break;
            };
            traceback = next;
        }
        result.omitted_frames = requested - retained.len();
        if result.omitted_frames > 0 {
            result.omissions.push(format!(
                "exception traceback budget: limit {FRAMES} frames per exception and {TOTAL_FRAMES} total, requested {requested} frames with {} remaining",
                self.frames,
            ));
        }
        self.frames -= retained.len();
        for frame in retained {
            let Ok(code) = frame
                .getattr("tb_frame")
                .and_then(|frame| frame.getattr("f_code"))
            else {
                continue;
            };
            if let (Ok(file), Ok(function), Ok(line)) = (
                code.getattr("co_filename"),
                code.getattr("co_name"),
                frame.getattr("tb_lineno").and_then(|line| line.extract()),
            ) {
                result.traceback.push(ExceptionFrame {
                    file: self.text(&file, &mut result.omissions),
                    line,
                    function: self.text(&function, &mut result.omissions),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_groups_text_and_cycles() -> PyResult<()> {
        Python::initialize();
        Python::attach(|py| {
            if py
                .import("builtins")?
                .getattr("BaseExceptionGroup")
                .is_err()
            {
                return Ok(());
            }
            let module = PyModule::from_code(
                py,
                c"
def wide():
    raise ExceptionGroup('wide', [ValueError(str(i)) for i in range(1000)])
def deep():
    error = ValueError('bottom')
    for _ in range(40):
        error = ExceptionGroup('nested', [error])
    raise error

def huge():
    error = ValueError('é' * 200000)
    error.add_note('note survives')
    raise ExceptionGroup('huge', [error, TypeError('sibling survives')])
def cyclic():
    error = ExceptionGroup('cycle', [ValueError('leaf')])
    error.__cause__ = error
    raise error
",
                c"groups.py",
                c"groups",
            )?;
            let error = module.getattr("wide")?.call0().expect_err("group");
            let wide = extract(py, &error).expect("group tree");
            assert_eq!(wide.children.len(), WIDTH);
            assert_eq!(wide.omitted_children, 1000 - WIDTH);
            assert!(wide.omissions[0].contains("width limit"));

            let error = module.getattr("deep")?.call0().expect_err("group");
            let mut deep = extract(py, &error).expect("group tree");
            let mut depth = 1;
            while let Some(child) = deep.children.pop() {
                depth += 1;
                deep = child;
            }
            assert_eq!(depth, DEPTH);
            assert_eq!(deep.omitted_children, 1);

            let error = module.getattr("huge")?.call0().expect_err("group");
            let huge = extract(py, &error).expect("group tree");
            assert!(huge.children[0].message.len() <= VALUE_BYTES);
            assert_eq!(huge.children[0].notes, ["note survives"]);
            assert_eq!(huge.children[1].message, "sibling survives");
            assert!(huge.children[0].omissions[0].contains("text budget"));

            let error = module.getattr("cyclic")?.call0().expect_err("group");
            let cyclic = extract(py, &error).expect("group tree");
            let cause = cyclic.cause.expect("cycle marker");
            assert!(cause.omissions[0].contains("cycle"));
            assert!(cause.cause.is_none());
            Ok(())
        })
    }
}

#[cfg(test)]
mod chain_tests {
    use super::*;

    #[test]
    fn finds_groups_wrapped_by_ordinary_exceptions() -> PyResult<()> {
        Python::initialize();
        Python::attach(|py| {
            let module = PyModule::from_code(
                py,
                c"
def wrap(explicit):
    try:
        raise ExceptionGroup('inner', [ValueError('leaf')])
    except ExceptionGroup as group:
        if explicit:
            raise RuntimeError('wrapper') from group
        raise RuntimeError('wrapper')
def suppressed():
    try:
        raise ExceptionGroup('hidden', [ValueError('leaf')])
    except ExceptionGroup:
        raise RuntimeError('wrapper') from None
",
                c"wrapped.py",
                c"wrapped",
            )?;
            let error = module.getattr("wrap")?.call1((true,)).expect_err("wrapper");
            let explicit = extract(py, &error).expect("wrapped group");
            assert_eq!(explicit.exception_type, "RuntimeError");
            assert_eq!(
                explicit.cause.expect("group cause").children[0].message,
                "leaf"
            );
            let error = module
                .getattr("wrap")?
                .call1((false,))
                .expect_err("wrapper");
            let implicit = extract(py, &error).expect("wrapped group");
            assert_eq!(
                implicit.context.expect("group context").children[0].message,
                "leaf"
            );
            let error = module.getattr("suppressed")?.call0().expect_err("wrapper");
            assert!(extract(py, &error).is_none());
            assert!(extract(py, &pyo3::exceptions::PyValueError::new_err("plain")).is_none());
            Ok(())
        })
    }

    #[test]
    fn retains_context_but_respects_suppression() -> PyResult<()> {
        Python::initialize();
        Python::attach(|py| {
            if py
                .import("builtins")?
                .getattr("BaseExceptionGroup")
                .is_err()
            {
                return Ok(());
            }
            let module = PyModule::from_code(
                py,
                c"
def context():
    try:
        raise KeyError('original')
    except KeyError:
        raise ExceptionGroup('context', [ValueError('leaf')])
def suppressed():
    try:
        raise KeyError('hidden')
    except KeyError:
        raise ExceptionGroup('suppressed', [ValueError('leaf')]) from None
",
                c"chains.py",
                c"chains",
            )?;
            let error = module.getattr("context")?.call0().expect_err("group");
            let context = extract(py, &error).expect("group");
            assert_eq!(
                context.context.expect("implicit context").exception_type,
                "KeyError"
            );
            let error = module.getattr("suppressed")?.call0().expect_err("group");
            let suppressed = extract(py, &error).expect("group");
            assert!(suppressed.context.is_none());
            assert!(suppressed.cause.is_none());
            Ok(())
        })
    }

    #[test]
    fn retains_raise_site_when_traceback_is_bounded() -> PyResult<()> {
        Python::initialize();
        Python::attach(|py| {
            if py
                .import("builtins")?
                .getattr("BaseExceptionGroup")
                .is_err()
            {
                return Ok(());
            }
            let module = PyModule::from_code(
                py,
                c"
def recurse(n):
    if n:
        return recurse(n - 1)
    raise ValueError('deep traceback')
def run():
    try:
        recurse(200)
    except ValueError as leaf:
        raise ExceptionGroup('traceback', [leaf]) from None
",
                c"frames.py",
                c"frames",
            )?;
            let error = module.getattr("run")?.call0().expect_err("group");
            let group = extract(py, &error).expect("group");
            let leaf = &group.children[0];
            assert_eq!(leaf.traceback.len(), FRAMES);
            assert_eq!(leaf.omitted_frames, 202 - FRAMES);
            assert_eq!(leaf.traceback.last().expect("raise site").line, 5);
            assert!(leaf.omissions[0].contains("traceback budget"));
            Ok(())
        })
    }
}
