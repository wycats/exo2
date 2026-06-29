use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::command_spec::ArgId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Value {
    Bool(bool),
    Int(i64),
    String(String),
    Path(String),
    Json(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invocation {
    /// Resolved command path (e.g. [`exo`, `list`, `tasks`]).
    pub path: Vec<String>,

    /// Typed values keyed by stable argument IDs.
    pub args: BTreeMap<ArgId, Value>,

    /// Occurrence counts for repeatable arguments.
    pub occurrences: BTreeMap<ArgId, u32>,
}

impl Invocation {
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(path: Vec<String>) -> Self {
        Self {
            path,
            args: BTreeMap::new(),
            occurrences: BTreeMap::new(),
        }
    }
}
