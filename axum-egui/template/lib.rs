use eframe::wasm_bindgen::{self, prelude::*};

#[wasm_bindgen]
pub fn start(canvas_id: &str) -> Result<(), eframe::wasm_bindgen::JsValue> {
    let app = APP_CREATOR.expect("APP_CREATOR must be set")();
    
    eframe::WebRunner::new()
        .start(
            canvas_id,
            Default::default(),
            Box::new(|_cc| Box::new(app)),
        )
}

// This will be set by the proc macro to create the app
static mut APP_CREATOR: Option<fn() -> Box<dyn eframe::App>> = None; 