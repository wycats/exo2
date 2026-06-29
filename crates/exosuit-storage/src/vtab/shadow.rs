//! xShadowName shim for protecting shadow tables.
//!
//! rusqlite 0.38 exposes `Module` as a transparent wrapper around
//! `sqlite3_module`, but does not expose a builder for xShadowName. Keep the
//! patch here so the raw SQLite ABI touchpoint stays small and local.

use super::reactive::ReactiveVTab;
use libsqlite3_sys as ffi;
use rusqlite::vtab::Module;
use std::ffi::{c_char, c_int, CStr};
use std::mem::{align_of, size_of};
use std::sync::OnceLock;

struct PatchedModule(ffi::sqlite3_module);

const _: () = {
    assert!(size_of::<Module<'static, ReactiveVTab>>() == size_of::<ffi::sqlite3_module>());
    assert!(align_of::<Module<'static, ReactiveVTab>>() == align_of::<ffi::sqlite3_module>());
};

// The patched sqlite3_module is initialized once and never mutated afterward.
unsafe impl Send for PatchedModule {}
unsafe impl Sync for PatchedModule {}

/// Check if a name is a shadow table suffix.
///
/// Shadow tables use these suffixes:
/// - `data` - The actual data storage (e.g., `epochs_data`)
/// - `rev` - Content revision digests (e.g., `epochs_rev`)
///
/// This is called by SQLite's xShadowName callback.
pub fn is_shadow_name(name: &str) -> bool {
    matches!(name, "data" | "rev")
}

/// Return a reactive module whose sqlite3_module advertises shadow tables.
///
/// Safety relies on rusqlite 0.38's `Module` being `#[repr(transparent)]` over
/// `sqlite3_module`; keep this helper specific to `ReactiveVTab` so any future
/// rusqlite upgrade has a single ABI assumption to review.
pub fn with_reactive_shadow_names<'vtab>(
    base: &'static Module<'vtab, ReactiveVTab>,
) -> &'static Module<'vtab, ReactiveVTab> {
    static PATCHED_REACTIVE_MODULE: OnceLock<PatchedModule> = OnceLock::new();

    let patched = PATCHED_REACTIVE_MODULE.get_or_init(|| {
        let base = base as *const Module<'vtab, ReactiveVTab> as *const ffi::sqlite3_module;
        // SAFETY: `Module` is `repr(transparent)` over `sqlite3_module` in
        // rusqlite 0.38. `sqlite3_module` is Copy, and the patched copy is kept
        // alive for the process lifetime by `OnceLock`.
        let mut module = unsafe { *base };
        module.iVersion = 3;
        module.xShadowName = Some(reactive_shadow_name);
        PatchedModule(module)
    });

    let module = &patched.0 as *const ffi::sqlite3_module as *const Module<'vtab, ReactiveVTab>;
    // SAFETY: The patched module has the same transparent layout as
    // rusqlite's `Module<ReactiveVTab>` and lives for the process lifetime.
    unsafe { &*module }
}

unsafe extern "C" fn reactive_shadow_name(name: *const c_char) -> c_int {
    if name.is_null() {
        return 0;
    }

    // SAFETY: SQLite passes a valid nul-terminated suffix string.
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return 0;
    };
    is_shadow_name(name) as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_shadow_name() {
        assert!(is_shadow_name("data"));
        assert!(is_shadow_name("rev"));
        assert!(!is_shadow_name("epochs"));
        assert!(!is_shadow_name("epochs_data")); // Full name, not suffix
        assert!(!is_shadow_name(""));
    }
}
