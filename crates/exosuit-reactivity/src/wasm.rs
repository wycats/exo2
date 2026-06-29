use crate::engine::Engine;
use crate::revision::{Epoch, Revision};
use crate::trace::{StateProvider, Trace, TraceDigest};
use crate::types::CellId;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;
use ulid::Ulid;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[wasm_bindgen]
pub struct WasmEngine {
    inner: Rc<RefCell<Engine>>,
    epoch: Epoch,
    counter: u64,
    scopes: HashMap<String, ScopeState>,
}

struct ScopeState {
    parent_id: Option<String>,
    trace: Trace,
}

impl Default for WasmEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl WasmEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        Self {
            inner: Rc::new(RefCell::new(Engine::new(Duration::from_secs(60)))),
            epoch: Epoch::new(),
            counter: 0,
            scopes: HashMap::new(),
        }
    }

    pub fn notify_file_change(&mut self, uri: &str) -> Result<JsValue, JsValue> {
        // Watcher events are invalidation triggers, not digest reads.
        // We do not read from disk here, and we do not assign new revisions.
        log(&format!("notify_file_change (invalidate): {}", uri));

        let mut affected_roots: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut engine = self.inner.borrow_mut();

        // Invalidate the path itself (source-scoped invalidation).
        let cell_id = CellId::root(uri);
        for root_id in engine.invalidate_cell(&cell_id) {
            affected_roots.insert(root_id);
        }

        // Invalidate ancestors so directory listings are marked stale.
        // This mirrors the behavior of rfs::FileSystem::notify_changed.
        let mut current = Path::new(uri).parent();
        while let Some(parent) = current {
            let parent_str = parent.to_string_lossy();
            let parent_id = CellId::root(parent_str.as_ref());
            for root_id in engine.invalidate_cell(&parent_id) {
                affected_roots.insert(root_id);
            }
            current = parent.parent();
        }

        let affected: Vec<String> = affected_roots.into_iter().collect();
        serde_wasm_bindgen::to_value(&affected)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
    }

    /// Validate a root digest against current state (consumes deps).
    pub fn validate_root(&mut self, id: &str, digest: &str) -> bool {
        self.inner.borrow_mut().validate_root(id, digest)
    }

    /// Get the current digest for a root, if present.
    pub fn get_root_digest(&self, id: &str) -> Option<String> {
        self.inner.borrow().get_root(id).map(|r| r.digest.clone())
    }

    /// Drop a root and unsubscribe it from invalidation.
    pub fn remove_root(&mut self, id: &str) {
        self.inner.borrow_mut().remove_root(id);
    }

    pub fn set_disk_revision(&mut self, uri: &str, hash: &str) {
        let cell_id = CellId::root(uri);
        let rev = Revision::disk(hash);
        self.inner.borrow_mut().set_cell(cell_id, rev);
    }

    /// Register a state root in the WASM engine with an initial Memory revision.
    ///
    /// Returns the initial revision as JSON (for the caller to use with
    /// `record_dependency` if needed).
    pub fn register_state_root(&mut self, key: &str) -> Result<String, JsValue> {
        self.counter += 1;
        let cell_id = CellId::root(key);
        let revision = Revision::memory(self.epoch, self.counter);
        self.inner.borrow_mut().set_cell(cell_id, revision.clone());
        serde_json::to_string(&revision)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
    }

    /// Bump a state root's revision.
    ///
    /// This updates the cell in-place (via `Engine::bump_cell`) rather than
    /// removing it (like `invalidate_cell`), so the revision remains queryable
    /// for subsequent `record_dependency` / `validate_trace` calls.
    ///
    /// The caller is responsible for firing `invalidateRoots([key])` on the
    /// TS side.  Staleness of derived roots is detected lazily by
    /// `validate_trace()`.
    pub fn bump_state_root(&mut self, key: &str) {
        self.counter += 1;
        let cell_id = CellId::root(key);
        let revision = Revision::memory(self.epoch, self.counter);
        self.inner.borrow_mut().bump_cell(cell_id, revision);
    }

    /// Get the current revision JSON for a state root cell.
    ///
    /// Returns `None` if the cell is not registered.
    pub fn get_state_revision(&self, key: &str) -> Option<String> {
        let cell_id = CellId::root(key);
        let engine = self.inner.borrow();
        engine
            .get_revision_snapshot(&cell_id)
            .and_then(|rev| serde_json::to_string(&rev).ok())
    }

    /// Compute a merkle hash for a *single-level* directory listing.
    ///
    /// Input is a JSON array of `{ name, kind, child_hash }` objects, where:
    /// - `kind` is one of: "file" | "dir" | "symlink"
    /// - `child_hash` is a hex SHA-256 digest for the child (or a constant for directories)
    ///
    /// This mirrors `rfs::DirectoryCell` hashing so the host and runtime agree.
    pub fn compute_directory_hash(&self, entries_json: &str) -> Result<String, JsValue> {
        #[derive(serde::Deserialize)]
        struct Entry {
            name: String,
            kind: String,
            child_hash: String,
        }

        let mut entries: Vec<Entry> = serde_json::from_str(entries_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid entries JSON: {e}")))?;

        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let mut hasher = Sha256::new();
        for entry in entries {
            hasher.update(entry.name.as_bytes());
            match entry.kind.as_str() {
                "file" => hasher.update(b"F"),
                "dir" => hasher.update(b"D"),
                "symlink" => hasher.update(b"L"),
                other => return Err(JsValue::from_str(&format!("Invalid entry kind: {other}"))),
            }
            hasher.update(entry.child_hash.as_bytes());
        }

        Ok(hex::encode(hasher.finalize()))
    }

    pub fn register_root(
        &mut self,
        id: &str,
        trace_json: &str,
        value_json: &str,
        is_subscriber: bool,
    ) -> Result<(), JsValue> {
        log(&format!("register_root called for id: {}", id));
        // Accept either:
        // - a full Trace JSON (preferred)
        // - a simple dependency list (strings or { path }) and build a Trace from current state
        let trace: Trace = match serde_json::from_str::<Trace>(trace_json) {
            Ok(trace) => trace,
            Err(_) => {
                #[derive(serde::Deserialize)]
                #[serde(untagged)]
                enum Dep {
                    Path(String),
                    Obj { path: String },
                }

                let deps: Vec<Dep> = serde_json::from_str(trace_json).map_err(|e| {
                    JsValue::from_str(&format!("Invalid dependency list JSON: {}", e))
                })?;

                let mut engine = self.inner.borrow_mut();
                let mut trace = Trace::new();
                for dep in deps {
                    let path = match dep {
                        Dep::Path(p) => p,
                        Dep::Obj { path } => path,
                    };

                    let cell_id = CellId::root(&path);
                    let revision = match engine.get_revision(&cell_id) {
                        Some(r) => r,
                        None => {
                            // If the host didn't ingest a disk revision, fall back to an epoch-scoped
                            // memory revision so the trace remains well-formed.
                            self.counter += 1;
                            let r = Revision::memory(self.epoch, self.counter);
                            engine.set_cell(cell_id.clone(), r.clone());
                            r
                        }
                    };

                    trace.record(cell_id, revision);
                }
                trace
            }
        };

        let value: serde_json::Value = serde_json::from_str(value_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid value JSON: {}", e)))?;

        log("register_root: parsed JSON");

        self.inner
            .borrow_mut()
            .register_root(id.to_string(), trace, value, is_subscriber);
        log("register_root: success");
        Ok(())
    }

    pub fn fetch_root(&self, id: &str, digest: &str) -> Result<String, JsValue> {
        let engine = self.inner.borrow();
        match engine.fetch_root(id, digest) {
            Ok(val) => serde_json::to_string(&val)
                .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e))),
            Err(e) => Err(JsValue::from_str(&format!("Fetch error: {}", e))),
        }
    }

    pub fn get_root_value(&self, id: &str) -> Result<String, JsValue> {
        let engine = self.inner.borrow();
        match engine.get_root(id) {
            Some(root) => serde_json::to_string(&root.value)
                .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e))),
            None => Err(JsValue::from_str("Root not found")),
        }
    }

    pub fn begin_transaction(&mut self) {
        self.inner.borrow_mut().begin_transaction();
    }

    pub fn end_transaction(&mut self) {
        self.inner.borrow_mut().end_transaction();
    }

    #[wasm_bindgen]
    pub fn begin_track(&mut self, parent_scope_id: Option<String>) -> String {
        let scope_id = Ulid::new().to_string();
        let state = ScopeState {
            parent_id: parent_scope_id,
            trace: Trace::new(),
        };
        self.scopes.insert(scope_id.clone(), state);
        scope_id
    }

    #[wasm_bindgen]
    pub fn record_dependency(
        &mut self,
        scope_id: &str,
        cell_id_json: &str,
        revision_json: &str,
    ) -> Result<(), JsValue> {
        let scope = self
            .scopes
            .get_mut(scope_id)
            .ok_or_else(|| JsValue::from_str(&format!("Scope not found: {}", scope_id)))?;

        let cell_id: CellId = serde_json::from_str(cell_id_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid cell_id: {}", e)))?;
        let revision: Revision = serde_json::from_str(revision_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid revision: {}", e)))?;

        scope.trace.record(cell_id, revision);
        Ok(())
    }

    #[wasm_bindgen]
    pub fn scope_read(&mut self, scope_id: &str, root_id: &str) -> Result<String, JsValue> {
        let scope = self
            .scopes
            .get_mut(scope_id)
            .ok_or_else(|| JsValue::from_str(&format!("Scope not found: {}", scope_id)))?;

        let root = {
            let engine = self.inner.borrow();
            let root = engine
                .get_root(root_id)
                .ok_or_else(|| JsValue::from_str(&format!("Root not found: {}", root_id)))?;
            root.clone()
        };

        // Per Iron Rule (reactivity.md §0): scopeRead is a Data Operation.
        // Staleness checking belongs in validate_trace (Metadata Operation).
        // scopeRead always returns current value and records the dependency.
        let cell_id = CellId::root(root_id);
        let revision = Revision::disk(root.digest.clone());
        scope.trace.record(cell_id, revision);

        let payload = serde_json::json!({
            "value": root.value.clone(),
            "digest": root.digest.clone(),
        });

        serde_json::to_string(&payload)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
    }

    #[wasm_bindgen]
    pub fn end_track(&mut self, scope_id: &str) -> Result<String, JsValue> {
        let scope = self
            .scopes
            .remove(scope_id)
            .ok_or_else(|| JsValue::from_str(&format!("Scope not found: {}", scope_id)))?;

        if let Some(parent_id) = &scope.parent_id {
            if let Some(parent) = self.scopes.get_mut(parent_id) {
                for dep in &scope.trace.dependencies {
                    parent
                        .trace
                        .record(dep.cell_id.clone(), dep.revision.clone());
                }
            }
        }

        serde_wasm_bindgen::to_value(&scope.trace)
            .map(|v| js_sys::JSON::stringify(&v).unwrap().as_string().unwrap())
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
    }

    #[wasm_bindgen]
    pub fn get_scope_revision(&self, scope_id: &str) -> Result<String, JsValue> {
        let scope = self
            .scopes
            .get(scope_id)
            .ok_or_else(|| JsValue::from_str(&format!("Scope not found: {}", scope_id)))?;
        Ok(scope.trace.digest())
    }

    #[wasm_bindgen]
    pub fn validate_trace(&self, trace_json: &str) -> Result<bool, JsValue> {
        let trace: Trace = serde_json::from_str(trace_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid trace JSON: {}", e)))?;
        Ok(trace.validate(&mut *self.inner.borrow_mut()))
    }
}
