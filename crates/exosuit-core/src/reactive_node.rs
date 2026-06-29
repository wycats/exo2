use exosuit_reactivity::{Availability, CellId, Runtime};
use serde_json::Value;
use std::rc::Rc;
use uuid::Uuid;

use std::fmt;

struct InnerNode<'a> {
    id: CellId,
    runtime: &'a Runtime,
}

impl<'a> fmt::Debug for InnerNode<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InnerNode").field("id", &self.id).finish()
    }
}

impl<'a> Drop for InnerNode<'a> {
    fn drop(&mut self) {
        self.runtime.remove_cell(&self.id);
    }
}

#[derive(Clone, Debug)]
pub struct ReactiveNode<'a> {
    inner: Rc<InnerNode<'a>>,
}

impl<'a> ReactiveNode<'a> {
    pub fn new<T: Into<Value>>(runtime: &'a Runtime, value: T) -> Self {
        let id_str = Uuid::new_v4().to_string();
        let cell = runtime.cell(&id_str, value.into());
        Self {
            inner: Rc::new(InnerNode {
                id: cell.id().clone(),
                runtime,
            }),
        }
    }

    pub fn id(&self) -> &CellId {
        &self.inner.id
    }

    pub fn get<T: serde::de::DeserializeOwned>(&self) -> T {
        let cell = self
            .inner
            .runtime
            .get_cell(&self.inner.id)
            .expect("ReactiveNode lost from Runtime");
        match cell.get() {
            Availability::Present(val) => serde_json::from_value(val).expect("Type mismatch"),
            Availability::Absent(_) => panic!("ReactiveNode value absent"),
        }
    }
}
