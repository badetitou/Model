//! The `wasm-bindgen` face of [`Engine`]: the contract fluid loads (see its
//! `docs/guides/browser-analysis.md`).

use wasm_bindgen::prelude::*;

use crate::Engine;

fn js(e: anyhow::Error) -> JsError {
    JsError::new(&format!("{e:#}"))
}

/// `new ModelSession()`, `add_file(path, content)` for each file, `build()`, then `execute(json)`.
#[wasm_bindgen]
pub struct ModelSession(Engine);

#[wasm_bindgen]
impl ModelSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> ModelSession {
        ModelSession(Engine::new())
    }

    pub fn add_file(&mut self, path: String, content: String) {
        self.0.add_file(path, content);
    }

    pub fn build(&mut self) -> Result<(), JsError> {
        self.0.build().map_err(js)
    }

    /// Warnings of `build`, one per line.
    pub fn warnings(&self) -> String {
        self.0.warnings().join("\n")
    }

    pub fn execute(&self, request_json: &str) -> Result<String, JsError> {
        self.0.execute(request_json).map_err(js)
    }
}

impl Default for ModelSession {
    fn default() -> Self {
        Self::new()
    }
}
