pub use exosuit_reactivity_core::{ResourceSpec, StateProvider, Trace, TraceEntry};

use exosuit_reactivity_core::Revision;
use sha2::{Digest, Sha256};

/// Extension trait providing the Merkle digest for traces.
///
/// This lives in `exosuit-reactivity` (not core) because it depends on
/// `sha2`, `hex`, and `bincode` — heavy deps that the core crate avoids.
pub trait TraceDigest {
    /// Calculates the Trace Digest (Merkle hash of dependencies).
    /// $D_{total} = Hash(D_{dependency_1} + D_{dependency_2} + \dots)$
    fn digest(&self) -> String;
}

impl TraceDigest for Trace {
    fn digest(&self) -> String {
        let mut hasher = Sha256::new();

        for entry in &self.dependencies {
            // Hash the CellId
            let mut entry_hasher = Sha256::new();
            entry_hasher.update(entry.cell_id.source_id.as_bytes());
            entry_hasher.update(entry.cell_id.pointer.as_bytes());

            // Hash the Revision
            match &entry.revision {
                Revision::Memory { epoch, counter } => {
                    entry_hasher.update(b"mem");
                    entry_hasher.update(
                        bincode::serde::encode_to_vec(epoch, bincode::config::standard())
                            .unwrap_or_default(),
                    );
                    entry_hasher.update(counter.to_le_bytes());
                }
                Revision::Disk { hash } => {
                    entry_hasher.update(b"disk");
                    entry_hasher.update(hash.as_bytes());
                }
                Revision::Counter(value) => {
                    entry_hasher.update(b"counter");
                    entry_hasher.update(value.to_le_bytes());
                }
                Revision::Impure { hash, nonce } => {
                    entry_hasher.update(b"impure");
                    entry_hasher.update(hash.as_bytes());
                    entry_hasher.update(nonce.to_le_bytes());
                }
            }

            hasher.update(entry_hasher.finalize());
        }

        // Hash Resources
        for resource in &self.resources {
            let mut res_hasher = Sha256::new();
            res_hasher.update(resource.type_id.as_bytes());
            res_hasher.update(resource.args.to_string().as_bytes());
            hasher.update(res_hasher.finalize());
        }

        hex::encode(hasher.finalize())
    }
}
