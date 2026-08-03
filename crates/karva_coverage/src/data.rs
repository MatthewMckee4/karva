//! Per-worker JSON schema. Both the tracer and the report side use these
//! types so the wire format stays in lockstep.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
/// Coverage payload persisted by one worker, keyed by normalized source path.
pub struct WorkerFile {
    /// Canonical source roots resolved by the worker's Python environment.
    #[serde(default)]
    pub source_roots: BTreeSet<String>,

    /// Per-source coverage collected by the worker.
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
/// Executable and observed coverage for one Python source file.
pub struct FileEntry {
    /// Sorted executable source-line numbers.
    pub executable: Vec<u32>,

    /// Executable source lines removed by exclusion rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<u32>,

    /// Source-line numbers observed at runtime.
    pub executed: Vec<u32>,

    /// Test contexts that executed each source line.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contexts: BTreeMap<u32, BTreeSet<String>>,

    /// Branch coverage, absent when branch tracing was disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<BranchEntry>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
/// Directed control-flow edge between source lines.
pub struct BranchArc {
    /// Origin line, or coverage.py's negative function-entry sentinel.
    pub from: i32,

    /// Destination line, or coverage.py's negative function-exit sentinel.
    pub to: i32,
}

#[derive(Debug, Serialize, Deserialize)]
/// Possible and executed control-flow edges for one source file.
pub struct BranchEntry {
    /// Statically discovered branch edges.
    pub possible: Vec<BranchArc>,

    /// Branch edges observed at runtime.
    pub executed: Vec<BranchArc>,

    /// Branch source lines whose unobserved destinations are intentional.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partial: Vec<u32>,

    /// Test contexts grouped by executed edge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<BranchContextEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
/// Test contexts that executed one branch edge.
pub struct BranchContextEntry {
    /// Executed control-flow edge.
    pub arc: BranchArc,

    /// Qualified test names that traversed the edge.
    pub contexts: BTreeSet<String>,
}
