//! Per-worker JSON schema. Both the tracer and the report side use these
//! types so the wire format stays in lockstep.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerFile {
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub executable: Vec<u32>,
    pub executed: Vec<u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contexts: BTreeMap<u32, BTreeSet<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<BranchEntry>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct BranchArc {
    pub from: i32,
    pub to: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BranchEntry {
    pub possible: Vec<BranchArc>,
    pub executed: Vec<BranchArc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<BranchContextEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BranchContextEntry {
    pub arc: BranchArc,
    pub contexts: BTreeSet<String>,
}
