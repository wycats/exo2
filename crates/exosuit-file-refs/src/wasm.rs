use crate::model::{ParseError, Surface};
use crate::present::present_paths;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmFileRefs;

#[wasm_bindgen]
impl WasmFileRefs {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        Self
    }

    pub fn present_paths(
        &self,
        workspace_root: &str,
        surface: &str,
        paths_json: &str,
    ) -> Result<JsValue, JsValue> {
        let surface = match surface {
            "webview" => Surface::Webview,
            "tree" => Surface::Tree,
            other => {
                return Err(JsValue::from_str(
                    &ParseError::InvalidSurface(other.to_string()).to_string(),
                ));
            }
        };

        let paths: Vec<String> = serde_json::from_str(paths_json)
            .map_err(|e| JsValue::from_str(&ParseError::InvalidJson(e.to_string()).to_string()))?;

        let tokens = present_paths(workspace_root, surface, paths);
        serde_wasm_bindgen::to_value(&tokens)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {e}")))
    }
}
