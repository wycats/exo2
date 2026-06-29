use exosuit_reactivity::{Availability, CellId, Runtime};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;
use uuid::Uuid;

struct InnerMap<'a, K, V> {
    structure_id: CellId,
    values: RefCell<HashMap<K, CellId>>,
    runtime: &'a Runtime,
    _marker: std::marker::PhantomData<V>,
}

impl<'a, K, V> fmt::Debug for InnerMap<'a, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InnerMap")
            .field("structure_id", &self.structure_id)
            .finish()
    }
}

impl<'a, K, V> Drop for InnerMap<'a, K, V> {
    fn drop(&mut self) {
        self.runtime.remove_cell(&self.structure_id);
        for id in self.values.borrow().values() {
            self.runtime.remove_cell(id);
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReactiveMap<'a, K, V> {
    inner: Rc<InnerMap<'a, K, V>>,
}

impl<'a, K, V> ReactiveMap<'a, K, V>
where
    K: Serialize + serde::de::DeserializeOwned + Clone + Hash + Eq + 'static,
    V: Serialize + serde::de::DeserializeOwned + Clone + 'static,
{
    pub fn new(runtime: &'a Runtime) -> Self {
        let id_str = Uuid::new_v4().to_string();
        // Structure cell tracks keys presence/absence
        let structure_cell =
            runtime.cell(&format!("{}_structure", id_str), serde_json::Value::Null);

        Self {
            inner: Rc::new(InnerMap {
                structure_id: structure_cell.id().clone(),
                values: RefCell::new(HashMap::new()),
                runtime,
                _marker: std::marker::PhantomData,
            }),
        }
    }

    pub fn len(&self) -> usize {
        self.read_structure();
        self.inner.values.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn insert(&self, key: K, value: V) -> Option<V> {
        let mut values = self.inner.values.borrow_mut();

        if let Some(cell_id) = values.get(&key) {
            // Key exists: Update value cell only
            let cell = self.inner.runtime.get_cell(cell_id).expect("Cell lost");

            // Get old value for return and equality check
            let old_json = match cell.get() {
                Availability::Present(v) => Some(v),
                _ => None,
            };

            let new_json = serde_json::to_value(&value).unwrap();

            // Optimization: Check equality to avoid invalidation
            if let Some(old) = &old_json
                && *old == new_json
            {
                return serde_json::from_value(old.clone()).ok();
            }

            cell.set(new_json);

            old_json.and_then(|v| serde_json::from_value(v).ok())
        } else {
            // Key new: Create cell, update structure
            let id_str = Uuid::new_v4().to_string();
            let cell = self
                .inner
                .runtime
                .cell(&id_str, serde_json::to_value(value).unwrap());
            values.insert(key, cell.id().clone());

            self.notify_structure();
            None
        }
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        let mut values = self.inner.values.borrow_mut();

        if let Some(cell_id) = values.remove(key) {
            let cell = self.inner.runtime.get_cell(&cell_id).expect("Cell lost");

            let old_val = match cell.get() {
                Availability::Present(v) => serde_json::from_value(v).ok(),
                _ => None,
            };

            // We remove the cell from runtime to clean up
            self.inner.runtime.remove_cell(&cell_id);

            self.notify_structure();
            old_val
        } else {
            None
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let values = self.inner.values.borrow();

        if let Some(cell_id) = values.get(key) {
            // Key exists: Read value cell
            let cell = self.inner.runtime.get_cell(cell_id).expect("Cell lost");
            match cell.get() {
                Availability::Present(val) => serde_json::from_value(val).ok(),
                _ => None,
            }
        } else {
            // Key missing: Read structure to track absence
            self.read_structure();
            None
        }
    }

    pub fn contains_key(&self, key: &K) -> bool {
        let values = self.inner.values.borrow();
        if values.contains_key(key) {
            // Key exists: We know it exists.
            // To track removal, we must read structure.
            // If we don't read structure, and it gets removed, we won't know.
            // Reading structure is coarse but correct.
            self.read_structure();
            true
        } else {
            // Key missing: Read structure to track insertion.
            self.read_structure();
            false
        }
    }

    pub fn keys(&self) -> Vec<K> {
        self.read_structure();
        self.inner.values.borrow().keys().cloned().collect()
    }

    pub fn entries(&self) -> Vec<(K, V)> {
        self.read_structure(); // Depend on structure
        let values = self.inner.values.borrow();
        let mut entries = Vec::new();
        for (k, cell_id) in values.iter() {
            let cell = self.inner.runtime.get_cell(cell_id).expect("Cell lost");
            // Reading the cell registers dependency
            if let Availability::Present(val) = cell.get()
                && let Ok(v) = serde_json::from_value(val)
            {
                entries.push((k.clone(), v));
            }
        }
        entries
    }

    pub fn to_hashmap(&self) -> HashMap<K, V> {
        self.read_structure(); // Dependency on set of keys
        let values = self.inner.values.borrow();
        let mut map = HashMap::new();

        for (k, cell_id) in values.iter() {
            let cell = self.inner.runtime.get_cell(cell_id).expect("Cell lost");
            if let Availability::Present(val) = cell.get()
                && let Ok(v) = serde_json::from_value(val)
            {
                map.insert(k.clone(), v);
            }
        }
        map
    }

    fn read_structure(&self) {
        let cell = self
            .inner
            .runtime
            .get_cell(&self.inner.structure_id)
            .expect("Structure cell lost");
        let _ = cell.get();
    }

    fn notify_structure(&self) {
        let cell = self
            .inner
            .runtime
            .get_cell(&self.inner.structure_id)
            .expect("Structure cell lost");
        // We just need to change the revision. Value doesn't matter.
        // We can use a counter or random value.
        cell.set(serde_json::Value::Null);
    }
}
