use std::collections::HashMap;
use std::io::Cursor;

use uesave::{Save, SaveReader, StructType, Types};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Parse a `.sav` file to JSON.
///
/// `hints_json` is a JSON object mapping dot-joined property paths to uesave
/// `StructType` names (same format the CLI accepts via `-t path=Type`). Pass
/// `""` (or `"{}"`) when no hints are needed. Missing hints cause uesave to
/// guess `Guid` for map keys and `Struct(None)` for values — correct for
/// standard engine types, often wrong for game-specific custom structs.
///
/// Semantics of the type name string match `StructType::from(&str)`:
///   - Known variant name ("DateTime", "Vector", "GameplayTagContainer", ...) → that variant
///   - "Struct"                                                               → Struct(None)
///   - anything else                                                          → Struct(Some(name))
#[wasm_bindgen]
pub fn sav_to_json(data: &[u8], hints_json: &str) -> Result<String, JsValue> {
    let mut types = Types::new();
    if !hints_json.trim().is_empty() {
        let hints: HashMap<String, String> = serde_json::from_str(hints_json)
            .map_err(|e| JsValue::from_str(&format!("invalid hints: {e}")))?;
        for (path, t) in hints {
            types.add(path, StructType::from(t.as_str()));
        }
    }

    let save = SaveReader::new()
        .error_to_raw(true)
        .types(types)
        .read(Cursor::new(data))
        .map_err(|e| JsValue::from_str(&format!("{e}")))?;

    serde_json::to_string(&save).map_err(|e| JsValue::from_str(&format!("{e}")))
}

#[wasm_bindgen]
pub fn json_to_sav(json: &str) -> Result<Vec<u8>, JsValue> {
    let save: Save = serde_json::from_str(json).map_err(|e| JsValue::from_str(&format!("{e}")))?;

    let mut buffer = Vec::new();
    save.write(&mut buffer)
        .map_err(|e| JsValue::from_str(&format!("{e}")))?;

    Ok(buffer)
}
