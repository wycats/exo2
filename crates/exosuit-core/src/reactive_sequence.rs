use exosuit_reactivity::{Availability, CellId, Runtime};
use serde::Serialize;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use uuid::Uuid;

struct InnerSequence<'a, T> {
    structure_id: CellId,
    values: RefCell<Vec<CellId>>,
    runtime: &'a Runtime,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T> fmt::Debug for InnerSequence<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InnerSequence")
            .field("structure_id", &self.structure_id)
            .finish()
    }
}

impl<'a, T> Drop for InnerSequence<'a, T> {
    fn drop(&mut self) {
        self.runtime.remove_cell(&self.structure_id);
        for id in self.values.borrow().iter() {
            self.runtime.remove_cell(id);
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReactiveSequence<'a, T> {
    inner: Rc<InnerSequence<'a, T>>,
}

impl<'a, T: Serialize + serde::de::DeserializeOwned + Clone + 'static> ReactiveSequence<'a, T> {
    pub fn new(runtime: &'a Runtime) -> Self {
        let id_str = Uuid::new_v4().to_string();
        // Structure cell tracks length/shape
        let structure_cell =
            runtime.cell(&format!("{}_structure", id_str), serde_json::Value::Null);

        Self {
            inner: Rc::new(InnerSequence {
                structure_id: structure_cell.id().clone(),
                values: RefCell::new(Vec::new()),
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

    pub fn push(&self, item: T) {
        let id_str = Uuid::new_v4().to_string();
        let cell = self
            .inner
            .runtime
            .cell(&id_str, serde_json::to_value(item).unwrap());

        self.inner.values.borrow_mut().push(cell.id().clone());
        self.notify_structure();
    }

    pub fn get(&self, index: usize) -> Option<T> {
        let values = self.inner.values.borrow();

        if let Some(cell_id) = values.get(index) {
            // Index exists: Read value cell
            let cell = self.inner.runtime.get_cell(cell_id).expect("Cell lost");
            match cell.get() {
                Availability::Present(val) => serde_json::from_value(val).ok(),
                _ => None,
            }
        } else {
            // Index out of bounds: Read structure to track growth
            self.read_structure();
            None
        }
    }

    pub fn set(&self, index: usize, value: T) {
        let values = self.inner.values.borrow();

        if let Some(cell_id) = values.get(index) {
            let cell = self.inner.runtime.get_cell(cell_id).expect("Cell lost");

            // Get old value for equality check
            let old_json = match cell.get() {
                Availability::Present(v) => Some(v),
                _ => None,
            };

            let new_json = serde_json::to_value(&value).unwrap();

            // Optimization: Check equality
            if let Some(old) = &old_json
                && *old == new_json
            {
                return;
            }

            cell.set(new_json);
        } else {
            panic!(
                "index out of bounds: the len is {} but the index is {}",
                values.len(),
                index
            );
        }
    }

    pub fn to_vec(&self) -> Vec<T> {
        self.read_structure(); // Depend on structure
        let values = self.inner.values.borrow();
        let mut result = Vec::with_capacity(values.len());

        for cell_id in values.iter() {
            let cell = self.inner.runtime.get_cell(cell_id).expect("Cell lost");
            if let Availability::Present(val) = cell.get()
                && let Ok(v) = serde_json::from_value(val)
            {
                result.push(v);
            }
        }
        result
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
        cell.set(serde_json::Value::Null);
    }
}
