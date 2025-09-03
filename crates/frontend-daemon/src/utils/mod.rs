pub mod favicon;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = "
export function is_tauri_context() {
    return typeof window !== 'undefined' && window.__TAURI__ !== undefined;
}
")]
extern "C" {
    #[wasm_bindgen(js_name = is_tauri_context)]
    pub fn is_tauri() -> bool;
}
