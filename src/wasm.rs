use crate::{PlayerConfig, PlayerEngine, PlayerEvent};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmPlayerEngine {
    inner: PlayerEngine,
}

#[wasm_bindgen]
impl WasmPlayerEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<WasmPlayerEngine, JsValue> {
        let config: PlayerConfig = if config.is_null() || config.is_undefined() {
            PlayerConfig::default()
        } else {
            serde_wasm_bindgen::from_value(config)?
        };
        Ok(Self {
            inner: PlayerEngine::new(config),
        })
    }
    pub fn dispatch(&mut self, event: JsValue) -> Result<(), JsValue> {
        let event: PlayerEvent = serde_wasm_bindgen::from_value(event)?;
        self.inner
            .dispatch(event)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
    #[wasm_bindgen(js_name = drainCommands)]
    pub fn drain_commands(&mut self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.drain_commands()).map_err(Into::into)
    }
    pub fn state(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(self.inner.state()).map_err(Into::into)
    }
    pub fn view(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.view()).map_err(Into::into)
    }
    pub fn revision(&self) -> u64 {
        self.inner.revision()
    }
}
