pub use exosuit_reactivity_core::CellId;

use serde::{Deserialize, Serialize};

// Fix 1: The "NaN" Fix (Canonical Value Equality)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SafeFloat(pub f64);

impl PartialEq for SafeFloat {
    fn eq(&self, other: &Self) -> bool {
        if self.0.is_nan() && other.0.is_nan() {
            true
        } else {
            self.0 == other.0
        }
    }
}

impl Eq for SafeFloat {}

impl std::hash::Hash for SafeFloat {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.0.is_nan() {
            // Canonical NaN hash
            state.write_u64(0x7ff8000000000000);
        } else {
            // Use byte representation for hashing
            state.write_u64(self.0.to_bits());
        }
    }
}
