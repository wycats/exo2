//! WASM bindings for ULID utilities.

use crate::ulid_util::{
    format_canonical_ref, generate_ulid, is_valid_ulid, parse_canonical_ref, parse_ulid,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Result of parsing a canonical reference.
#[derive(Serialize, Deserialize)]
pub struct CanonicalRef {
    /// The type name (e.g., "phase", "task", "epoch").
    #[serde(rename = "typeName")]
    pub type_name: String,
    /// The ULID string.
    pub ulid: String,
}

/// WASM bindings for ULID operations.
#[wasm_bindgen]
pub struct WasmUlid;

#[wasm_bindgen]
impl WasmUlid {
    /// Create a new WasmUlid instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        Self
    }

    /// Generate a new ULID.
    #[wasm_bindgen(js_name = generateUlid)]
    pub fn generate_ulid(&self) -> String {
        generate_ulid().to_string()
    }

    /// Format a ULID as a canonical reference (e.g., "phase@01HZ...").
    #[wasm_bindgen(js_name = formatCanonicalRef)]
    pub fn format_canonical_ref(&self, type_name: &str, ulid: &str) -> Result<String, JsValue> {
        let parsed =
            parse_ulid(ulid).ok_or_else(|| JsValue::from_str(&format!("Invalid ULID: {ulid}")))?;
        Ok(format_canonical_ref(type_name, &parsed))
    }

    /// Parse a ULID string, returning null if invalid.
    #[wasm_bindgen(js_name = parseUlid)]
    pub fn parse_ulid(&self, s: &str) -> Option<String> {
        parse_ulid(s).map(|u| u.to_string())
    }

    /// Check if a string is a valid ULID.
    #[wasm_bindgen(js_name = isValidUlid)]
    pub fn is_valid_ulid(&self, s: &str) -> bool {
        is_valid_ulid(s)
    }

    /// Parse a canonical reference (e.g., "phase@01HZ...").
    /// Returns null if invalid, otherwise returns { typeName, ulid }.
    #[wasm_bindgen(js_name = parseCanonicalRef)]
    pub fn parse_canonical_ref(&self, s: &str) -> Result<JsValue, JsValue> {
        match parse_canonical_ref(s) {
            Some((type_name, ulid)) => {
                let result = CanonicalRef {
                    type_name,
                    ulid: ulid.to_string(),
                };
                serde_wasm_bindgen::to_value(&result)
                    .map_err(|e| JsValue::from_str(&format!("Serialization error: {e}")))
            }
            None => Ok(JsValue::NULL),
        }
    }
}

impl Default for WasmUlid {
    fn default() -> Self {
        Self::new()
    }
}
