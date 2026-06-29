use crate::availability::{Availability, Reason};
use crate::engine::Engine;
use crate::revision::{Epoch, Revision};
use crate::trace::{ResourceSpec, StateProvider, Trace, TraceDigest};
use crate::types::CellId;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;
use tokio::time::Instant;

thread_local! {
    static ACTIVE_TRACE: RefCell<Vec<Trace>> = const { RefCell::new(Vec::new()) };
}

// --- Resource Traits ---

pub trait Resource {
    fn init(args: &serde_json::Value) -> Self
    where
        Self: Sized;
    fn value(&self) -> serde_json::Value;
}

trait AnyResource {
    fn value(&self) -> serde_json::Value;
    #[allow(dead_code)]
    fn as_any(&self) -> &dyn Any;
}

impl<T: Resource + 'static> AnyResource for T {
    fn value(&self) -> serde_json::Value {
        self.value()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

type ActiveResources = HashMap<String, Vec<(ResourceSpec, Box<dyn AnyResource>)>>;
type ResourceRegistry = HashMap<String, Box<dyn Fn(&serde_json::Value) -> Box<dyn AnyResource>>>;

struct ResourceManager {
    // Map RootId -> List of (Spec, Instance)
    active: ActiveResources,
    // Registry: TypeId -> FactoryFn
    registry: ResourceRegistry,
}

impl ResourceManager {
    fn new() -> Self {
        Self {
            active: HashMap::new(),
            registry: HashMap::new(),
        }
    }
}

// --- Runtime ---

pub struct Runtime {
    engine: RefCell<Engine>,
    store: RefCell<HashMap<CellId, serde_json::Value>>,
    epoch: RefCell<Epoch>,
    counter: RefCell<u64>,
    resources: RefCell<ResourceManager>,

    // Context Stacks for Resource Hook-like behavior
    current_computation: RefCell<Vec<String>>,
    resource_cursor: RefCell<Vec<usize>>,

    // Hysteresis: Notification channel for background timers
    invalidation_tx: RefCell<Option<tokio::sync::mpsc::UnboundedSender<String>>>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            engine: RefCell::new(Engine::new(Duration::from_secs(60))),
            store: RefCell::new(HashMap::new()),
            epoch: RefCell::new(Epoch::new()),
            counter: RefCell::new(0),
            resources: RefCell::new(ResourceManager::new()),
            current_computation: RefCell::new(Vec::new()),
            resource_cursor: RefCell::new(Vec::new()),
            invalidation_tx: RefCell::new(None),
        }
    }

    pub fn set_invalidation_sender(&self, tx: tokio::sync::mpsc::UnboundedSender<String>) {
        *self.invalidation_tx.borrow_mut() = Some(tx);
    }

    pub fn active_cell_count(&self) -> usize {
        self.store.borrow().len()
    }

    pub fn remove_cell(&self, id: &CellId) {
        self.store.borrow_mut().remove(id);
        let affected_roots = self.engine.borrow_mut().invalidate_cell(id);

        // Notify affected roots
        if let Some(tx) = &*self.invalidation_tx.borrow() {
            for root_id in affected_roots {
                let _ = tx.send(root_id);
            }
        }
    }

    pub fn invalidate(&self, id: &str) {
        let cell_id = CellId::root(id);
        self.remove_cell(&cell_id);
    }

    pub fn register_resource<R: Resource + 'static>(&self, type_id: &str) {
        let factory = Box::new(|args: &serde_json::Value| -> Box<dyn AnyResource> {
            Box::new(R::init(args))
        });
        self.resources
            .borrow_mut()
            .registry
            .insert(type_id.to_string(), factory);
    }

    pub fn cell(&self, id: &str, value: serde_json::Value) -> Cell<'_> {
        let cell_id = CellId::root(id);
        self.store.borrow_mut().insert(cell_id.clone(), value);

        let mut counter = self.counter.borrow_mut();
        *counter += 1;
        let rev = Revision::memory(*self.epoch.borrow(), *counter);

        self.engine.borrow_mut().set_cell(cell_id.clone(), rev);

        Cell {
            id: cell_id,
            runtime: self,
        }
    }

    pub fn get_cell(&self, id: &CellId) -> Option<Cell<'_>> {
        if self.store.borrow().contains_key(id) {
            Some(Cell {
                id: id.clone(),
                runtime: self,
            })
        } else {
            None
        }
    }

    pub fn computed<F>(&self, id: &str, compute: F) -> Computed<'_, F>
    where
        F: Fn() -> Availability<serde_json::Value>,
    {
        Computed {
            id: id.to_string(),
            compute,
            runtime: self,
            last_digest: RefCell::new(None),
            last_value: RefCell::new(None),
            hysteresis: None,
            last_valid_time: RefCell::new(None),
            last_present_value: RefCell::new(None),
        }
    }

    pub fn action<F>(&self, f: F)
    where
        F: FnOnce(),
    {
        f();
    }

    // Fix 2: The "Async Drop" Fix (Deferred Cleanup)
    pub fn defer_cleanup<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        // We use tokio::spawn to offload cleanup tasks.
        // This is the *only* exception to the "No Detached Tasks" rule.
        // In a production environment, this should be bounded by a Semaphore.
        tokio::spawn(future);
    }

    pub fn resource(
        &self,
        type_id: &str,
        args: serde_json::Value,
    ) -> Availability<serde_json::Value> {
        // 1. Record in Trace
        let spec = ResourceSpec {
            type_id: type_id.to_string(),
            args: args.clone(),
        };

        ACTIVE_TRACE.with(|t| {
            if let Some(trace) = t.borrow_mut().last_mut() {
                trace.record_resource(spec.clone());
            }
        });

        // 2. Lookup existing instance
        // We need to know which computation we are in and the cursor index.
        let comp_stack = self.current_computation.borrow();
        let cursor_stack = self.resource_cursor.borrow();

        if !comp_stack.is_empty() && !cursor_stack.is_empty() {
            // Increment cursor for next call
            // We need to drop the borrow to mutate.
            drop(comp_stack);
            drop(cursor_stack);

            let comp_id = self.current_computation.borrow().last().unwrap().clone();
            let cursor = *self.resource_cursor.borrow().last().unwrap();
            *self.resource_cursor.borrow_mut().last_mut().unwrap() += 1;

            let resources = self.resources.borrow();
            if let Some(active_list) = resources.active.get(&comp_id)
                && let Some((old_spec, instance)) = active_list.get(cursor)
                && old_spec.type_id == type_id
                && old_spec.args == args
            {
                return Availability::Present(instance.value());
            }
        }

        // If not found or mismatch, return Loading (will be inited in Commit phase)
        Availability::Absent(Reason::Loading)
    }

    fn reconcile(&self, id: &str, trace: &Trace) {
        let mut resources = self.resources.borrow_mut();

        // Take ownership of old resources
        let old_resources = resources.active.remove(id).unwrap_or_default();
        let mut old_iter = old_resources.into_iter();
        let mut final_resources = Vec::new();

        for spec in &trace.resources {
            let old = old_iter.next();

            let instance = match old {
                Some((old_spec, old_inst)) if old_spec == *spec => {
                    // Match! Keep it.
                    old_inst
                }
                _ => {
                    // Mismatch or New. Create new.
                    if let Some(factory) = resources.registry.get(&spec.type_id) {
                        factory(&spec.args)
                    } else {
                        panic!("Resource type not found: {}", spec.type_id);
                    }
                }
            };
            final_resources.push((spec.clone(), instance));
        }

        // Remaining items in `old_iter` are dropped (disposed).

        resources.active.insert(id.to_string(), final_resources);
    }
}

pub struct Cell<'a> {
    id: CellId,
    runtime: &'a Runtime,
}

impl<'a> Cell<'a> {
    pub fn id(&self) -> &CellId {
        &self.id
    }

    pub fn get(&self) -> Availability<serde_json::Value> {
        ACTIVE_TRACE.with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(active) = stack.last_mut() {
                let mut engine = self.runtime.engine.borrow_mut();
                if let Some(rev) = engine.get_revision(&self.id) {
                    active.record(self.id.clone(), rev);
                }
            }
        });

        let val = self
            .runtime
            .store
            .borrow()
            .get(&self.id)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        Availability::Present(val)
    }

    pub fn set(&self, value: serde_json::Value) {
        self.runtime
            .store
            .borrow_mut()
            .insert(self.id.clone(), value);

        let mut counter = self.runtime.counter.borrow_mut();
        *counter += 1;
        let rev = Revision::memory(*self.runtime.epoch.borrow(), *counter);

        self.runtime
            .engine
            .borrow_mut()
            .set_cell(self.id.clone(), rev);
    }
}

pub struct Computed<'a, F> {
    id: String,
    compute: F,
    runtime: &'a Runtime,
    last_digest: RefCell<Option<String>>,
    last_value: RefCell<Option<Availability<serde_json::Value>>>,
    hysteresis: Option<Duration>,
    last_valid_time: RefCell<Option<Instant>>,
    last_present_value: RefCell<Option<serde_json::Value>>,
}

impl<'a, F> Computed<'a, F>
where
    F: Fn() -> Availability<serde_json::Value>,
{
    pub fn with_hysteresis(mut self, duration: Duration) -> Self {
        self.hysteresis = Some(duration);
        self
    }

    pub fn read(&self) -> Availability<serde_json::Value> {
        let is_valid = {
            if let Some(digest) = &*self.last_digest.borrow() {
                self.runtime
                    .engine
                    .borrow_mut()
                    .validate_root(&self.id, digest)
            } else {
                false
            }
        };

        let value = if is_valid {
            // Replay dependencies from the cached trace
            if let Some(root) = self.runtime.engine.borrow().get_root(&self.id) {
                ACTIVE_TRACE.with(|stack| {
                    if let Some(active) = stack.borrow_mut().last_mut() {
                        for dep in &root.trace.dependencies {
                            active.record(dep.cell_id.clone(), dep.revision.clone());
                        }
                    }
                });
            }
            self.last_value.borrow().clone().unwrap()
        } else {
            ACTIVE_TRACE.with(|stack| stack.borrow_mut().push(Trace::new()));

            // Setup Resource Context
            self.runtime
                .current_computation
                .borrow_mut()
                .push(self.id.clone());
            self.runtime.resource_cursor.borrow_mut().push(0);

            let value = (self.compute)();

            self.runtime.current_computation.borrow_mut().pop();
            self.runtime.resource_cursor.borrow_mut().pop();

            let trace = ACTIVE_TRACE.with(|stack| stack.borrow_mut().pop().unwrap());

            // Replay dependencies to the parent trace (Flattening)
            ACTIVE_TRACE.with(|stack| {
                if let Some(active) = stack.borrow_mut().last_mut() {
                    for dep in &trace.dependencies {
                        active.record(dep.cell_id.clone(), dep.revision.clone());
                    }
                }
            });

            // Reconcile Resources
            self.runtime.reconcile(&self.id, &trace);

            let digest = trace.digest();
            let value_json = serde_json::to_value(&value).unwrap_or(serde_json::Value::Null);
            self.runtime.engine.borrow_mut().register_root(
                self.id.clone(),
                trace,
                value_json,
                false,
            ); // Computed is NOT a subscriber

            *self.last_digest.borrow_mut() = Some(digest);
            *self.last_value.borrow_mut() = Some(value.clone());

            value
        };

        // Fix 3: Hysteresis (Self-Healing Timers - Logic Part)

        // Note: We already replayed dependencies above, so we don't need to record self.id
        // unless we want to track the computation itself as a node.
        // Given the "Flattening" requirement, we rely on the replayed cells.

        if let Availability::Absent(_) = &value {
            let mut use_stale = false;
            if let Some(duration) = self.hysteresis
                && let Some(last_time) = *self.last_valid_time.borrow()
                && last_time.elapsed() < duration
            {
                use_stale = true;
            }

            if use_stale {
                if let Some(old_val) = &*self.last_present_value.borrow() {
                    // Schedule a oneshot timer to force re-evaluation after grace period.
                    if let Some(tx) = &*self.runtime.invalidation_tx.borrow()
                        && let Some(duration) = self.hysteresis
                    {
                        let tx = tx.clone();
                        let id = self.id.clone();
                        let remaining = duration
                            .saturating_sub(self.last_valid_time.borrow().unwrap().elapsed());

                        tokio::spawn(async move {
                            tokio::time::sleep(remaining).await;
                            let _ = tx.send(id);
                        });
                    }

                    Availability::Present(old_val.clone())
                } else {
                    value
                }
            } else {
                value
            }
        } else {
            // Update last_present_value if Present
            if let Availability::Present(v) = &value {
                *self.last_valid_time.borrow_mut() = Some(Instant::now());
                *self.last_present_value.borrow_mut() = Some(v.clone());
            }
            value
        }
    }
}

impl<'a, F> Drop for Computed<'a, F> {
    fn drop(&mut self) {
        self.runtime.engine.borrow_mut().remove_root(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::available;
    use serde_json::json;

    // Mock Resource
    struct MockFetcher {
        url: String,
    }

    impl Resource for MockFetcher {
        fn init(args: &serde_json::Value) -> Self {
            Self {
                url: args.as_str().unwrap().to_string(),
            }
        }

        fn value(&self) -> serde_json::Value {
            json!(format!("Fetched: {}", self.url))
        }
    }

    #[test]
    fn test_resource_lifecycle() {
        let runtime = Runtime::new();
        runtime.register_resource::<MockFetcher>("fetch");

        let url_cell = runtime.cell("url", json!("https://example.com"));

        let fetched = runtime.computed("fetched", || {
            let url = url_cell.get().unwrap();
            runtime.resource("fetch", url)
        });

        let res1 = fetched.read();
        assert!(matches!(res1, Availability::Absent(Reason::Loading)));

        runtime.action(|| {
            url_cell.set(json!("https://example.com")); // Same value, but new revision
        });

        let res3 = fetched.read();
        assert_eq!(
            res3,
            Availability::Present(json!("Fetched: https://example.com"))
        );

        runtime.action(|| {
            url_cell.set(json!("https://api.com"));
        });

        let res4 = fetched.read();
        assert!(matches!(res4, Availability::Absent(Reason::Loading)));

        runtime.action(|| {
            url_cell.set(json!("https://api.com"));
        });

        let res5 = fetched.read();
        assert_eq!(
            res5,
            Availability::Present(json!("Fetched: https://api.com"))
        );
    }

    #[test]
    fn test_resource_macro_dx() {
        let runtime = Runtime::new();
        runtime.register_resource::<MockFetcher>("fetch");
        let url_cell = runtime.cell("url", json!("https://example.com"));

        let fetched = runtime.computed("fetched", || {
            let url = url_cell.get().unwrap();
            // Use the macro!
            let val = available!(runtime.resource("fetch", url));
            Availability::Present(json!(format!("Processed: {}", val)))
        });

        // 1. Loading
        let res1 = fetched.read();
        assert!(matches!(res1, Availability::Absent(Reason::Loading)));

        // 2. Ready
        runtime.action(|| {
            url_cell.set(json!("https://example.com"));
        });
        let res2 = fetched.read();
        assert_eq!(
            res2,
            Availability::Present(json!("Processed: \"Fetched: https://example.com\""))
        );
    }

    #[test]
    fn test_hello_world_dx() {
        let runtime = Runtime::new();
        let cell = runtime.cell("user-name", json!("Alice"));
        let greeting = runtime.computed("greeting", || {
            let name = cell.get().unwrap();
            Availability::Present(json!(format!("Hello, {}!", name.as_str().unwrap())))
        });
        assert_eq!(
            greeting.read(),
            Availability::Present(json!("Hello, Alice!"))
        );
        runtime.action(|| {
            cell.set(json!("Bob"));
        });
        assert_eq!(greeting.read(), Availability::Present(json!("Hello, Bob!")));
    }
}
