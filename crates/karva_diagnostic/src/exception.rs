//! Bounded, Python-independent exception trees carried across worker IPC.

use std::fmt::Write;

use camino::Utf8Path;

use serde::{Deserialize, Serialize};

/// A retained group, leaf, or chained exception. Children preserve Python order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionDiagnostic {
    pub exception_type: String,

    pub message: String,

    pub notes: Vec<String>,

    /// Frames run from the outermost call to the raise site.
    pub traceback: Vec<ExceptionFrame>,

    pub children: Vec<Self>,

    /// Direct children excluded by a width, depth, or shared budget.
    pub omitted_children: usize,

    /// Traceback frames excluded from the beginning of this traceback.
    pub omitted_frames: usize,

    pub cause: Option<Box<Self>>,

    /// Implicit context, absent when Python suppresses it or supplies a cause.
    pub context: Option<Box<Self>>,

    /// Named budgets, their limits and requests, or a detected chain cycle.
    pub omissions: Vec<String>,
}

/// One retained Python frame, without source snapshots or Python references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionFrame {
    pub file: String,

    pub line: usize,

    pub function: String,
}

impl ExceptionDiagnostic {
    /// Formats every retained node with its stable path, including chains and notes.
    pub(crate) fn render(&self, cwd: &Utf8Path) -> String {
        let mut output = String::new();
        self.write(&mut output, "exception", cwd);
        output
    }

    fn write(&self, output: &mut String, path: &str, cwd: &Utf8Path) {
        let _ = writeln!(output, "{path}: {}: {}", self.exception_type, self.message);
        let has_user_frames = self
            .traceback
            .iter()
            .any(|frame| Utf8Path::new(&frame.file).starts_with(cwd));
        for frame in &self.traceback {
            if has_user_frames && !Utf8Path::new(&frame.file).starts_with(cwd) {
                continue;
            }
            let _ = writeln!(
                output,
                "  at {}:{} in {}",
                frame.file, frame.line, frame.function
            );
        }
        if self.omitted_frames > 0 {
            let _ = writeln!(
                output,
                "  {} earlier traceback frames hidden",
                self.omitted_frames
            );
        }
        for note in &self.notes {
            let _ = writeln!(output, "  note: {note}");
        }
        for omission in &self.omissions {
            let _ = writeln!(output, "  {omission}");
        }
        if let Some(cause) = &self.cause {
            cause.write(output, &format!("{path}.cause"), cwd);
        }
        if let Some(context) = &self.context {
            context.write(output, &format!("{path}.context"), cwd);
        }
        for (index, child) in self.children.iter().enumerate() {
            child.write(output, &format!("{path}.{}", index + 1), cwd);
        }
        if self.omitted_children > 0 {
            let _ = writeln!(
                output,
                "  {path}: {} children hidden",
                self.omitted_children
            );
        }
    }
}
