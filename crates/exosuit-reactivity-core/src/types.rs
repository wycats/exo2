use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct CellId {
    pub source_id: String,
    pub pointer: String,
}

impl CellId {
    pub fn new(source_id: impl Into<String>, pointer: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            pointer: pointer.into(),
        }
    }

    pub fn root(source_id: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            pointer: String::new(),
        }
    }
}
