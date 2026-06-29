use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Epoch(Uuid);

impl Epoch {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create an Epoch from an existing UUID.
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Access the inner UUID.
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for Epoch {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Revision {
    /// Epoch-scoped monotonic counter for in-memory state.
    Memory { epoch: Epoch, counter: u64 },
    /// Content hash for persistent state (files, row digests).
    Disk { hash: String },
    /// Persistent monotonic counter (row-set membership revisions).
    Counter(u64),
    /// Impure computation result (Generative Identity).
    /// The hash is derived from Trace + Nonce.
    Impure { hash: String, nonce: u64 },
}

impl Revision {
    pub const fn memory(epoch: Epoch, counter: u64) -> Self {
        Self::Memory { epoch, counter }
    }

    pub fn disk(hash: impl Into<String>) -> Self {
        Self::Disk { hash: hash.into() }
    }

    pub const fn counter(value: u64) -> Self {
        Self::Counter(value)
    }

    pub fn impure(hash: impl Into<String>, nonce: u64) -> Self {
        Self::Impure {
            hash: hash.into(),
            nonce,
        }
    }
}
