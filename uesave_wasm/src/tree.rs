//! A parsed save kept in WASM memory, queried by path, so callers can read the parts they
//! show without converting the whole save to JSON.
//!
//! Paths follow the shape of `sav_to_json`'s output: a JSON array of segments, where a
//! property is named `<name>_<index>` (`"SaveData_0"`), array, set and map items are
//! numbers, and a map item has `"key"` and `"value"`. `[]` is the root properties.

use std::io::Cursor;

use serde::Serialize;
use serde_json::{json, Value};
use uesave::{ByteArray, MapEntry, Properties, Property, Save, StructValue, ValueVec};
use wasm_bindgen::prelude::*;

use crate::{open_compressed, parse_paths, read_save, to_js};

#[wasm_bindgen]
pub struct ParsedSave {
    save: Save,
}

#[wasm_bindgen]
impl ParsedSave {
    /// Parse a `.sav` file. `hints_json` is as for `sav_to_json`; `paths_json` is `""`
    /// to read everything, or a JSON array of paths to read only those (as for
    /// `sav_to_json_only`).
    #[wasm_bindgen(constructor)]
    pub fn new(data: &[u8], hints_json: &str, paths_json: &str) -> Result<ParsedSave, JsValue> {
        let save = read_save(Cursor::new(data), hints_json, optional_paths(paths_json)?)?;
        Ok(Self { save })
    }

    /// Parse a save stored compressed (see `compressed_sav_to_json`).
    #[wasm_bindgen(js_name = fromCompressed)]
    pub fn from_compressed(
        compressed: &[u8],
        hints_json: &str,
        paths_json: &str,
    ) -> Result<ParsedSave, JsValue> {
        let save = read_save(
            open_compressed(compressed)?,
            hints_json,
            optional_paths(paths_json)?,
        )?;
        Ok(Self { save })
    }

    /// The save's type, such as `/Script/ProjectCoral.C_SaveGame`
    #[wasm_bindgen(getter, js_name = saveGameType)]
    pub fn save_game_type(&self) -> String {
        self.save.root.save_game_type.clone()
    }

    /// The whole save as JSON, the same as `sav_to_json`
    #[wasm_bindgen(js_name = toJson)]
    pub fn to_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.save).map_err(to_js)
    }

    /// The value at `path` as JSON
    pub fn json(&self, path_json: &str) -> Result<String, JsValue> {
        let node = self.find(path_json)?;
        let json = match node {
            Node::Properties(p) => serde_json::to_string(p),
            Node::Property(p) => serde_json::to_string(p),
            Node::Struct(s) => serde_json::to_string(s),
            Node::Entry(e) => serde_json::to_string(e),
            Node::Leaf(v) => Ok(v.to_string()),
        };
        json.map_err(to_js)
    }

    /// The children of the value at `path`, without their contents: a JSON array of
    /// `{ key, kind, len }`, where `key` is the next path segment, `kind` names the
    /// value's type, and `len` counts its children (absent for plain values). Plain
    /// values also include `value`.
    pub fn keys(&self, path_json: &str) -> Result<String, JsValue> {
        let children: Vec<Value> = match self.find(path_json)? {
            Node::Properties(props) => props
                .0
                .iter()
                .map(|(key, value)| describe(json!(format!("{}_{}", key.1, key.0)), value))
                .collect(),
            Node::Property(p) => match p {
                Property::Struct(s) => struct_children(s),
                Property::Array(v) | Property::Set(v) => vec_children(v),
                Property::Map(entries) => entries
                    .iter()
                    .enumerate()
                    .map(|(i, _)| json!({ "key": i, "kind": "MapEntry", "len": 2 }))
                    .collect(),
                _ => vec![],
            },
            Node::Struct(s) => struct_children(s),
            Node::Entry(e) => vec![
                describe(json!("key"), &e.key),
                describe(json!("value"), &e.value),
            ],
            Node::Leaf(_) => vec![],
        };
        serde_json::to_string(&children).map_err(to_js)
    }

    fn find(&self, path_json: &str) -> Result<Node<'_>, JsValue> {
        let path: Vec<Value> = if path_json.trim().is_empty() {
            vec![]
        } else {
            serde_json::from_str(path_json).map_err(|e| to_js(format!("invalid path: {e}")))?
        };
        let mut node = Node::Properties(&self.save.root.properties);
        for (depth, segment) in path.iter().enumerate() {
            node = node.child(segment).ok_or_else(|| {
                to_js(format!(
                    "no {segment} at {}",
                    Value::from(path[..depth].to_vec())
                ))
            })?;
        }
        Ok(node)
    }
}

enum Node<'a> {
    Properties(&'a Properties),
    Property(&'a Property),
    Struct(&'a StructValue),
    Entry(&'a MapEntry),
    /// An element of a plain-value array, serialized on the way down
    Leaf(Value),
}

impl<'a> Node<'a> {
    fn child(self, segment: &Value) -> Option<Node<'a>> {
        match self {
            Node::Properties(props) => {
                let name = segment.as_str()?;
                props
                    .0
                    .iter()
                    .find(|(key, _)| format!("{}_{}", key.1, key.0) == name)
                    .map(|(_, value)| Node::Property(value))
            }
            Node::Property(Property::Struct(s)) => Node::Struct(s).child(segment),
            Node::Property(Property::Array(v) | Property::Set(v)) => {
                vec_item(v, segment.as_u64()? as usize)
            }
            Node::Property(Property::Map(entries)) => {
                entries.get(segment.as_u64()? as usize).map(Node::Entry)
            }
            Node::Struct(StructValue::Struct(props)) => Node::Properties(props).child(segment),
            Node::Entry(entry) => match segment.as_str()? {
                "key" => Some(Node::Property(&entry.key)),
                "value" => Some(Node::Property(&entry.value)),
                _ => None,
            },
            _ => None,
        }
    }
}

fn vec_item(values: &ValueVec, index: usize) -> Option<Node<'_>> {
    match values {
        ValueVec::Struct(items) => items.get(index).map(Node::Struct),
        other => to_json_value(other)
            .ok()?
            .as_array()?
            .get(index)
            .cloned()
            .map(Node::Leaf),
    }
}

fn vec_children(values: &ValueVec) -> Vec<Value> {
    match values {
        ValueVec::Struct(items) => items
            .iter()
            .enumerate()
            .map(|(i, s)| describe_struct(json!(i), s))
            .collect(),
        other => match to_json_value(other) {
            Ok(Value::Array(items)) => items
                .into_iter()
                .enumerate()
                .map(|(i, value)| json!({ "key": i, "kind": "Value", "value": value }))
                .collect(),
            _ => vec![],
        },
    }
}

fn struct_children(s: &StructValue) -> Vec<Value> {
    match s {
        StructValue::Struct(props) => props
            .0
            .iter()
            .map(|(key, value)| describe(json!(format!("{}_{}", key.1, key.0)), value))
            .collect(),
        _ => vec![],
    }
}

fn describe(key: Value, property: &Property) -> Value {
    let (kind, len) = match property {
        Property::Struct(s) => return describe_struct(key, s),
        Property::Array(v) => ("Array", Some(vec_len(v))),
        Property::Set(v) => ("Set", Some(vec_len(v))),
        Property::Map(entries) => ("Map", Some(entries.len())),
        Property::Raw(bytes) => ("Raw", Some(bytes.len())),
        _ => ("Value", None),
    };
    match len {
        Some(len) => json!({ "key": key, "kind": kind, "len": len }),
        None => json!({ "key": key, "kind": kind, "value": to_value(property) }),
    }
}

fn describe_struct(key: Value, s: &StructValue) -> Value {
    match s {
        StructValue::Struct(props) => json!({ "key": key, "kind": "Struct", "len": props.0.len() }),
        other => json!({ "key": key, "kind": "Value", "value": to_value(other) }),
    }
}

fn vec_len(values: &ValueVec) -> usize {
    match values {
        ValueVec::Int8(v) => v.len(),
        ValueVec::Int16(v) => v.len(),
        ValueVec::Int(v) => v.len(),
        ValueVec::Int64(v) => v.len(),
        ValueVec::UInt8(v) => v.len(),
        ValueVec::UInt16(v) => v.len(),
        ValueVec::UInt32(v) => v.len(),
        ValueVec::UInt64(v) => v.len(),
        ValueVec::Float(v) => v.len(),
        ValueVec::Double(v) => v.len(),
        ValueVec::Bool(v) => v.len(),
        ValueVec::Byte(ByteArray::Byte(v)) => v.len(),
        ValueVec::Byte(ByteArray::Label(v)) => v.len(),
        ValueVec::Enum(v) => v.len(),
        ValueVec::Str(v) => v.len(),
        ValueVec::Text(v) => v.len(),
        ValueVec::SoftObject(v) => v.len(),
        ValueVec::Name(v) => v.len(),
        ValueVec::Object(v) => v.len(),
        ValueVec::Box(v) => v.len(),
        ValueVec::Box2D(v) => v.len(),
        ValueVec::Struct(v) => v.len(),
    }
}

fn to_value(value: &impl Serialize) -> Value {
    to_json_value(value).unwrap_or(Value::Null)
}

/// Convert through the JSON text so values print exactly as in `toJson`.
/// (`serde_json::to_value` widens `f32`s, so `28812.867` would print as `28812.8671875`.)
fn to_json_value(value: &impl Serialize) -> serde_json::Result<Value> {
    serde_json::from_str(&serde_json::to_string(value)?)
}

fn optional_paths(paths_json: &str) -> Result<Option<Vec<String>>, JsValue> {
    if paths_json.trim().is_empty() {
        Ok(None)
    } else {
        parse_paths(paths_json).map(Some)
    }
}
