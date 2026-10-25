use std::collections::HashMap;
use std::io::{Cursor, Read, Seek};

use uesave::{
    ByteArray, CompressedReader, Property, Save, SaveReader, StructType, Types, ValueVec,
};
use wasm_bindgen::prelude::*;

mod tree;
pub use tree::ParsedSave;

/// The hint value that decodes a byte array as a property list (see `sav_to_json`).
const NESTED_PROPERTIES: &str = "Properties";

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
///
/// One more value isn't a struct type: `"Properties"` on a byte array property (a
/// `TArray<uint8>`) decodes its bytes as a property list, for games that store a struct's
/// tagged properties in one. It then reads as an object of properties instead of
/// `{"Byte": [...]}`, and the save can't be written back with `json_to_sav`. Bytes that
/// don't parse as a property list are kept as they are.
#[wasm_bindgen]
pub fn sav_to_json(data: &[u8], hints_json: &str) -> Result<String, JsValue> {
    read_to_json(Cursor::new(data), hints_json, None)
}

/// Parse only some properties of a `.sav` file to JSON.
///
/// `paths_json` is a JSON array of dot-joined property paths, in the same form as
/// the hint paths (`"SaveData.playerInfo"`). Those properties, everything inside
/// them and the properties containing them are read; all others are skipped by
/// their tag size without being parsed, and are left out of the result. Much
/// faster than `sav_to_json` when only a few values are needed.
#[wasm_bindgen]
pub fn sav_to_json_only(
    data: &[u8],
    hints_json: &str,
    paths_json: &str,
) -> Result<String, JsValue> {
    read_to_json(
        Cursor::new(data),
        hints_json,
        Some(parse_paths(paths_json)?),
    )
}

/// Like `sav_to_json`, for a save stored compressed: `compressed` is a UE compressed
/// archive (zlib chunks, as `FArchiveLoadCompressedProxy` reads) holding the save as a
/// length-prefixed byte array (`TArray<uint8>`).
#[wasm_bindgen]
pub fn compressed_sav_to_json(compressed: &[u8], hints_json: &str) -> Result<String, JsValue> {
    read_to_json(open_compressed(compressed)?, hints_json, None)
}

/// Like `sav_to_json_only`, for a save stored compressed (see `compressed_sav_to_json`).
/// Only the compressed chunks holding the properties that are read get inflated, so
/// this is much faster than decompressing the whole save first.
#[wasm_bindgen]
pub fn compressed_sav_to_json_only(
    compressed: &[u8],
    hints_json: &str,
    paths_json: &str,
) -> Result<String, JsValue> {
    read_to_json(
        open_compressed(compressed)?,
        hints_json,
        Some(parse_paths(paths_json)?),
    )
}

/// The bytes of a top-level byte array property, such as a compressed save payload,
/// without converting the save to JSON. Only that property is parsed.
#[wasm_bindgen]
pub fn sav_property_bytes(data: &[u8], name: &str) -> Result<Vec<u8>, JsValue> {
    let save = SaveReader::new()
        .only([name])
        .read_seekable(Cursor::new(data))
        .map_err(to_js)?;
    let property = save
        .root
        .properties
        .0
        .iter()
        .find(|(key, _)| key.1 == name)
        .map(|(_, property)| property)
        .ok_or_else(|| JsValue::from_str(&format!("no property named '{name}'")))?;
    match property {
        Property::Array(ValueVec::Byte(ByteArray::Byte(bytes))) => Ok(bytes.clone()),
        _ => Err(JsValue::from_str(&format!(
            "property '{name}' is not a byte array"
        ))),
    }
}

fn open_compressed(compressed: &[u8]) -> Result<CompressedReader<'_>, JsValue> {
    let mut stream = CompressedReader::new(compressed).map_err(to_js)?;
    let mut prefix = [0; 4];
    stream.read_exact(&mut prefix).map_err(to_js)?;
    let len = u32::from_le_bytes(prefix) as u64;
    stream.window(4, len).map_err(to_js)
}

fn parse_paths(paths_json: &str) -> Result<Vec<String>, JsValue> {
    serde_json::from_str(paths_json).map_err(|e| JsValue::from_str(&format!("invalid paths: {e}")))
}

fn read_to_json<S: Read + Seek>(
    stream: S,
    hints_json: &str,
    paths: Option<Vec<String>>,
) -> Result<String, JsValue> {
    serde_json::to_string(&read_save(stream, hints_json, paths)?).map_err(to_js)
}

fn read_save<S: Read + Seek>(
    stream: S,
    hints_json: &str,
    paths: Option<Vec<String>>,
) -> Result<Save, JsValue> {
    let mut types = Types::new();
    let mut nested = vec![];
    if !hints_json.trim().is_empty() {
        let hints: HashMap<String, String> = serde_json::from_str(hints_json)
            .map_err(|e| JsValue::from_str(&format!("invalid hints: {e}")))?;
        for (path, t) in hints {
            if t == NESTED_PROPERTIES {
                nested.push(path);
            } else {
                types.add(path, StructType::from(t.as_str()));
            }
        }
    }

    let mut reader = SaveReader::new().error_to_raw(true).types(types);
    if !nested.is_empty() {
        reader = reader.nested(nested);
    }
    if let Some(paths) = paths {
        reader = reader.only(paths);
    }
    reader.read_seekable(stream).map_err(to_js)
}

fn to_js(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&format!("{e}"))
}

#[wasm_bindgen]
pub fn json_to_sav(json: &str) -> Result<Vec<u8>, JsValue> {
    let save: Save = serde_json::from_str(json).map_err(|e| JsValue::from_str(&format!("{e}")))?;

    let mut buffer = Vec::new();
    save.write(&mut buffer)
        .map_err(|e| JsValue::from_str(&format!("{e}")))?;

    Ok(buffer)
}
