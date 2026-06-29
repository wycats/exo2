use crate::revision::Revision;
use crate::trace::{StateProvider, Trace, TraceDigest};
use crate::types::CellId;
// use im::HashMap as ImHashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use web_time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Root {
    pub trace: Trace,
    pub value: serde_json::Value,
    pub digest: String,
}

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("Root not found")]
    NotFound,
    #[error("Stale digest: expected {expected}, found {actual}")]
    Stale { expected: String, actual: String },
    #[error("Corrupted: Snapshot metadata exists but content is missing")]
    Corrupted,
}

#[derive(Debug, Clone)]
struct CellEntry {
    revision: Revision,
    last_used: u64,
}

pub struct Engine {
    // The current state of the world (structurally shared map)
    current_state: HashMap<CellId, CellEntry>,

    // History of states (snapshots), keyed by Root Digest.
    // Stores Root and creation timestamp.
    snapshots: HashMap<String, (Root, Instant)>,

    // Retention window for snapshots (Lazy TTL)
    retention_window: Duration,

    // Current Roots (latest)
    roots: HashMap<String, Root>,

    // Reverse Dependency Index: CellId -> Set<RootId>
    reverse_deps: HashMap<CellId, std::collections::HashSet<String>>,

    // Source Index: SourceID -> Set<CellId>
    // Used for Source-Scoped Invalidation (e.g. File Events)
    source_index: HashMap<String, std::collections::HashSet<CellId>>,

    // Monotonic Transaction ID (Epoch)
    current_transaction: u64,
}

impl Engine {
    pub fn new(retention_window: Duration) -> Self {
        Self {
            current_state: HashMap::new(),
            snapshots: HashMap::new(),
            retention_window,
            roots: HashMap::new(),
            reverse_deps: HashMap::new(),
            source_index: HashMap::new(),
            current_transaction: 1,
        }
    }

    pub fn begin_transaction(&mut self) {
        self.current_transaction += 1;
    }

    pub fn end_transaction(&mut self) {
        // GC: Remove cells not used in the current transaction
        // Note: We only GC if they are NOT roots (Roots are persistent until dropped)
        // Actually, Roots are stored in `roots`, not `current_state`.
        // `current_state` stores the Cells (Disk/Memory).

        // Optimization: Only GC periodically or if map grows too large?
        // For now, let's do it strictly as requested: "keep cells around if they're used in the last transaction"

        let current_tx = self.current_transaction;
        let mut to_remove = Vec::new();

        for (id, entry) in &self.current_state {
            if entry.last_used < current_tx {
                to_remove.push(id.clone());
            }
        }

        for id in to_remove {
            self.current_state.remove(&id);
            // Remove from source_index
            if let Some(cells) = self.source_index.get_mut(&id.source_id) {
                cells.remove(&id);
                if cells.is_empty() {
                    self.source_index.remove(&id.source_id);
                }
            }
            // We don't need to remove from reverse_deps because if it wasn't used,
            // it means no active root depends on it (or the root wasn't re-evaluated).
        }
    }

    pub fn set_cell(&mut self, cell_id: CellId, revision: Revision) {
        self.current_state.insert(
            cell_id.clone(),
            CellEntry {
                revision,
                last_used: self.current_transaction,
            },
        );
        // Update source_index
        self.source_index
            .entry(cell_id.source_id.clone())
            .or_default()
            .insert(cell_id);
    }

    /// Update a cell's revision in-place.
    ///
    /// Unlike `invalidate_cell()` (which removes the cell), this keeps the cell
    /// in `current_state` with the new revision, making it suitable for state
    /// roots that are mutated rather than invalidated.
    ///
    /// The caller is responsible for firing invalidation (e.g. via
    /// `invalidateRoots([key])` on the TS side).  Staleness of derived roots
    /// is detected lazily by `validate_trace()`.
    pub fn bump_cell(&mut self, cell_id: CellId, new_revision: Revision) {
        self.set_cell(cell_id, new_revision);
    }

    pub fn invalidate_cell(&mut self, cell_id: &CellId) -> Vec<String> {
        // If pointer is empty, it's a Source Invalidation (e.g. File Event)
        if cell_id.pointer.is_empty()
            && let Some(cells) = self.source_index.remove(&cell_id.source_id)
        {
            let mut affected_roots = std::collections::HashSet::new();
            for cell in cells {
                self.current_state.remove(&cell);
                if let Some(roots) = self.reverse_deps.get(&cell) {
                    affected_roots.extend(roots.iter().cloned());
                }
            }
            return affected_roots.into_iter().collect();
        }

        // Normal single cell invalidation
        self.current_state.remove(cell_id);

        // Return affected roots
        self.reverse_deps
            .get(cell_id)
            .map(|roots| roots.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn get_root(&self, root_id: &str) -> Option<&Root> {
        self.roots.get(root_id)
    }

    /// Get the current revision for a cell without updating `last_used` or
    /// performing recursive trace validation.
    ///
    /// This is a pure snapshot read, suitable for returning revision data to
    /// callers (e.g. TypeScript) that need to construct `record_dependency`
    /// arguments.
    pub fn get_revision_snapshot(&self, cell_id: &CellId) -> Option<Revision> {
        self.current_state.get(cell_id).map(|e| e.revision.clone())
    }

    pub fn remove_root(&mut self, root_id: &str) {
        if let Some(root) = self.roots.remove(root_id) {
            // Clean up reverse_deps
            for dep in &root.trace.dependencies {
                if let Some(roots) = self.reverse_deps.get_mut(&dep.cell_id) {
                    roots.remove(root_id);
                    if roots.is_empty() {
                        self.reverse_deps.remove(&dep.cell_id);
                    }
                }
            }
        }
    }

    pub fn register_root(
        &mut self,
        root_id: String,
        trace: Trace,
        value: serde_json::Value,
        is_subscriber: bool,
    ) {
        // Mark dependencies as used!
        for dep in &trace.dependencies {
            if let Some(entry) = self.current_state.get_mut(&dep.cell_id) {
                entry.last_used = self.current_transaction;
            }
        }

        // 1. Update Reverse Index (Only if subscriber)
        // First, remove old dependencies for this root (if any)
        if let Some(old_root) = self.roots.get(&root_id) {
            for dep in &old_root.trace.dependencies {
                if let Some(roots) = self.reverse_deps.get_mut(&dep.cell_id) {
                    roots.remove(&root_id);
                    if roots.is_empty() {
                        self.reverse_deps.remove(&dep.cell_id);
                    }
                }
            }
        }

        if is_subscriber {
            // Add new dependencies
            for dep in &trace.dependencies {
                self.reverse_deps
                    .entry(dep.cell_id.clone())
                    .or_default()
                    .insert(root_id.clone());
            }
        }

        let digest = trace.digest();
        // let digest = "dummy-digest".to_string();
        let root = Root {
            trace,
            value,
            digest: digest.clone(),
        };

        self.roots.insert(root_id.clone(), root.clone());

        // Treat the Root as a Cell so it can be depended upon
        let revision = Revision::Disk {
            hash: digest.clone(),
        };
        self.set_cell(CellId::root(root_id), revision);

        // Snapshot Pinning: Save the Root
        self.snapshots
            .insert(digest, (root.clone(), Instant::now()));

        // Lazy TTL Pruning: Scan and remove old snapshots
        self.prune_snapshots();
    }

    fn prune_snapshots(&mut self) {
        let now = Instant::now();
        self.snapshots
            .retain(|_, (_, timestamp)| now.duration_since(*timestamp) < self.retention_window);
    }

    pub fn validate_root(&mut self, root_id: &str, client_digest: &str) -> bool {
        if let Some(root) = self.roots.get(root_id)
            && root.digest == client_digest
        {
            // Even if digests match, we must validate the trace against current state
            let trace = root.trace.clone();
            return trace.validate(self);
        }
        false
    }

    pub fn fetch_root(
        &self,
        root_id: &str,
        expected_digest: &str,
    ) -> Result<serde_json::Value, FetchError> {
        // 1. Check if we have the snapshot for this digest
        if let Some((root, _)) = self.snapshots.get(expected_digest) {
            return Ok(root.value.clone());
        }

        // 2. Fallback to current roots if digest matches
        if let Some(root) = self.roots.get(root_id)
            && root.digest == expected_digest
        {
            return Ok(root.value.clone());
        }

        Err(FetchError::Stale {
            expected: expected_digest.to_string(),
            actual: self
                .roots
                .get(root_id)
                .map(|r| r.digest.clone())
                .unwrap_or_default(),
        })
    }
}

impl StateProvider for Engine {
    fn get_revision(&mut self, cell_id: &CellId) -> Option<Revision> {
        // 1. Check if it's a Root (Recursive Validation)
        if cell_id.pointer.is_empty() {
            // We must clone to avoid borrowing self while recursing
            if let Some(root) = self.roots.get(&cell_id.source_id).cloned() {
                // Check if the Root's trace is valid against current state
                if !root.trace.validate(self) {
                    return None; // The Root is stale, so we treat it as missing/changed
                }
            }
        }

        if let Some(entry) = self.current_state.get_mut(cell_id) {
            entry.last_used = self.current_transaction;
            return Some(entry.revision.clone());
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::revision::{Epoch, Revision};
    use crate::trace::{Trace, TraceDigest};
    use crate::types::CellId;
    use std::time::Instant;

    #[test]
    fn test_verifiable_fetch_with_snapshots() {
        let mut engine = Engine::new(Duration::from_secs(60));
        let epoch = Epoch::new();

        // Setup a cell
        let cell_id = CellId::root("test.txt");
        let rev1 = Revision::memory(epoch, 1);
        engine.set_cell(cell_id.clone(), rev1.clone());

        // Setup a root that depends on the cell
        let mut trace = Trace::new();
        trace.record(cell_id.clone(), rev1.clone());
        let value = serde_json::json!({"content": "v1"});
        engine.register_root("root1".to_string(), trace.clone(), value.clone(), true);

        let digest1 = trace.digest();

        // 1. Verify fetch works with correct digest
        let fetched = engine.fetch_root("root1", &digest1).unwrap();
        assert_eq!(fetched, value);

        // 2. Update cell (simulate change)
        let rev2 = Revision::memory(epoch, 2);
        engine.set_cell(cell_id.clone(), rev2.clone());

        // 3. Update root in engine (simulate recomputation)
        let mut trace2 = Trace::new();
        trace2.record(cell_id.clone(), rev2);
        let value2 = serde_json::json!({"content": "v2"});
        engine.register_root("root1".to_string(), trace2.clone(), value2, true);

        let digest2 = trace2.digest();

        // 4. Verify fetch with OLD digest still works (Snapshot Pinning!)
        let fetched_old = engine.fetch_root("root1", &digest1).unwrap();
        assert_eq!(fetched_old, value);

        // 5. Verify fetch with NEW digest works
        let fetched_new = engine.fetch_root("root1", &digest2).unwrap();
        assert_eq!(fetched_new, serde_json::json!({"content": "v2"}));
    }

    #[test]
    fn test_transaction_gc() {
        let mut engine = Engine::new(Duration::from_secs(60));
        let epoch = Epoch::new();

        let cell1 = CellId::root("used");
        let cell2 = CellId::root("unused");

        engine.set_cell(cell1.clone(), Revision::memory(epoch, 1));
        engine.set_cell(cell2.clone(), Revision::memory(epoch, 1));

        // Start Transaction 2
        engine.begin_transaction();

        // Access cell1
        engine.get_revision(&cell1);

        // End Transaction
        engine.end_transaction();

        // cell1 should be kept, cell2 removed
        assert!(engine.current_state.contains_key(&cell1));
        assert!(!engine.current_state.contains_key(&cell2));
    }

    #[test]
    fn test_snapshot_ttl_eviction() {
        // Retention window very short
        let mut engine = Engine::new(Duration::from_millis(10));
        let epoch = Epoch::new();

        let cell_id = CellId::root("test.txt");
        let rev1 = Revision::memory(epoch, 1);
        engine.set_cell(cell_id.clone(), rev1.clone());

        let mut trace = Trace::new();
        trace.record(cell_id.clone(), rev1.clone());
        let value = serde_json::json!({"content": "v1"});
        engine.register_root("root1".to_string(), trace.clone(), value.clone(), true);

        let digest1 = trace.digest();

        // Should be present
        assert!(engine.fetch_root("root1", &digest1).is_ok());

        // Wait for expiration
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(20) {
            std::hint::spin_loop();
        }

        // Trigger pruning (by registering a new root)
        let rev2 = Revision::memory(epoch, 2);
        let mut trace2 = Trace::new();
        trace2.record(cell_id.clone(), rev2);
        engine.register_root(
            "root1".to_string(),
            trace2,
            serde_json::json!({"content": "v2"}),
            true,
        );

        // Old snapshot should be gone
        // Note: fetch_root checks snapshots AND current roots.
        // Since we updated root1 to v2, checking for digest1 should fail if snapshot is gone.
        let result = engine.fetch_root("root1", &digest1);
        assert!(matches!(result, Err(FetchError::Stale { .. })));
    }

    #[test]
    fn test_bump_cell_updates_revision_in_place() {
        let mut engine = Engine::new(Duration::from_secs(60));
        let epoch = Epoch::new();

        // Setup a state cell
        let cell_id = CellId::root("state:ui.selectedPhase");
        let rev1 = Revision::memory(epoch, 1);
        engine.set_cell(cell_id.clone(), rev1.clone());

        // Register a derived root that depends on this state cell
        let mut trace = Trace::new();
        trace.record(cell_id.clone(), rev1.clone());
        let value = serde_json::json!({"selected": "phase-1"});
        engine.register_root("phaseDetails".to_string(), trace, value, true);

        // Bump the state cell
        let rev2 = Revision::memory(epoch, 2);
        engine.bump_cell(cell_id.clone(), rev2.clone());

        // Cell should still exist with new revision
        let snapshot = engine.get_revision_snapshot(&cell_id);
        assert!(snapshot.is_some());
        assert_eq!(snapshot.unwrap(), rev2);

        // Old revision should no longer match
        assert_ne!(rev1, rev2);
    }

    #[test]
    fn test_bump_cell_no_dependents() {
        let mut engine = Engine::new(Duration::from_secs(60));
        let epoch = Epoch::new();

        let cell_id = CellId::root("state:ui.orphaned");
        let rev1 = Revision::memory(epoch, 1);
        engine.set_cell(cell_id.clone(), rev1);

        let rev2 = Revision::memory(epoch, 2);
        engine.bump_cell(cell_id.clone(), rev2);

        // Cell should still exist with bumped revision
        assert!(engine.get_revision_snapshot(&cell_id).is_some());
    }

    #[test]
    fn test_get_revision_snapshot_returns_none_for_missing() {
        let engine = Engine::new(Duration::from_secs(60));
        let cell_id = CellId::root("state:nonexistent");
        assert!(engine.get_revision_snapshot(&cell_id).is_none());
    }
}
