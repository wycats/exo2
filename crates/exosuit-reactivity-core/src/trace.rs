use crate::revision::Revision;
use crate::types::CellId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceSpec {
    pub type_id: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct TraceEntry {
    pub cell_id: CellId,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Trace {
    pub dependencies: BTreeSet<TraceEntry>,
    pub resources: Vec<ResourceSpec>,
}

impl Trace {
    pub const fn new() -> Self {
        Self {
            dependencies: BTreeSet::new(),
            resources: Vec::new(),
        }
    }

    pub fn record(&mut self, cell_id: CellId, revision: Revision) {
        self.dependencies.insert(TraceEntry { cell_id, revision });
    }

    pub fn record_resource(&mut self, spec: ResourceSpec) {
        self.resources.push(spec);
    }

    pub fn entries(&self) -> impl Iterator<Item = &TraceEntry> {
        self.dependencies.iter()
    }

    pub fn validate(&self, state: &mut impl StateProvider) -> bool {
        for entry in &self.dependencies {
            match state.get_revision(&entry.cell_id) {
                Some(current_rev) => {
                    if current_rev != entry.revision {
                        return false;
                    }
                }
                None => return false, // Cell missing implies invalid
            }
        }
        true
    }
}

pub trait StateProvider {
    fn get_revision(&mut self, cell_id: &CellId) -> Option<Revision>;
}
