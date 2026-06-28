use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{NexDbError, NexDbResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document(Value);

impl Document {
    pub fn new() -> Self {
        Document(Value::Object(Map::new()))
    }

    pub fn from_json(json: &str) -> NexDbResult<Self> {
        let value: Value = serde_json::from_str(json)?;
        Ok(Document(value))
    }

    pub fn from_value(value: Value) -> Self {
        Document(value)
    }

    pub fn get(&self, field: &str) -> Option<&Value> {
        self.0.get(field)
    }

    pub fn get_path(&self, path: &str) -> Option<&Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = &self.0;
        for part in parts {
            current = current.get(part)?;
        }
        Some(current)
    }

    pub fn set(&mut self, field: &str, value: Value) -> Option<Value> {
        if self.0.is_object() {
            self.0.as_object_mut()
                .and_then(|map| map.insert(field.to_string(), value))
        } else {
            let mut map = Map::new();
            map.insert(field.to_string(), value);
            self.0 = Value::Object(map);
            None
        }
    }

    pub fn remove(&mut self, field: &str) -> Option<Value> {
        self.0.as_object_mut()?.remove(field)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.0).unwrap_or_default()
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.0).unwrap_or_default()
    }

    pub fn into_inner(self) -> Value {
        self.0
    }

    pub fn as_value(&self) -> &Value {
        &self.0
    }

    pub fn is_object(&self) -> bool {
        self.0.is_object()
    }

    pub fn fields(&self) -> Vec<String> {
        self.0.as_object()
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_json_pretty())
    }
}

impl FromStr for Document {
    type Err = NexDbError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Document::from_json(s)
    }
}

impl From<Value> for Document {
    fn from(value: Value) -> Self {
        Document(value)
    }
}

impl From<Document> for Value {
    fn from(doc: Document) -> Self {
        doc.0
    }
}

#[cfg(feature = "bson")]
impl Document {
    pub fn to_bson(&self) -> bson::Document {
        let value: &Value = &self.0;
        bson_value_to_document(value)
    }

    pub fn from_bson(doc: &bson::Document) -> Self {
        Document(bson_document_to_json(doc))
    }
}

#[cfg(feature = "bson")]
fn bson_value_to_document(value: &Value) -> bson::Document {
    match value {
        Value::Object(map) => {
            let mut bdoc = bson::Document::new();
            for (k, v) in map {
                bdoc.insert(k.as_str(), json_to_bson_element(v));
            }
            bdoc
        }
        _ => {
            let mut bdoc = bson::Document::new();
            bdoc.insert("value", json_to_bson_element(value));
            bdoc
        }
    }
}

#[cfg(feature = "bson")]
fn json_to_bson_element(value: &Value) -> bson::Bson {
    match value {
        Value::Null => bson::Bson::Null,
        Value::Bool(b) => bson::Bson::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() { return bson::Bson::Int64(i); }
            if let Some(f) = n.as_f64() { return bson::Bson::Double(f); }
            bson::Bson::Int64(0)
        }
        Value::String(s) => bson::Bson::String(s.clone()),
        Value::Array(arr) => {
            let items: Vec<bson::Bson> = arr.iter().map(|v| json_to_bson_element(v)).collect();
            bson::Bson::Array(items)
        }
        Value::Object(map) => {
            let mut bdoc = bson::Document::new();
            for (k, v) in map {
                bdoc.insert(k.as_str(), json_to_bson_element(v));
            }
            bson::Bson::Document(bdoc)
        }
    }
}

#[cfg(feature = "bson")]
fn bson_document_to_json(bdoc: &bson::Document) -> Value {
    let mut map = Map::new();
    for (k, v) in bdoc {
        map.insert(k.clone(), bson_to_json_value(v));
    }
    Value::Object(map)
}

#[cfg(feature = "bson")]
fn bson_to_json_value(bson: &bson::Bson) -> Value {
    match bson {
        bson::Bson::Null => Value::Null,
        bson::Bson::Boolean(b) => Value::Bool(*b),
        bson::Bson::Int32(i) => Value::Number((*i).into()),
        bson::Bson::Int64(i) => Value::Number((*i).into()),
        bson::Bson::Double(f) => {
            serde_json::Number::from_f64(*f)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        bson::Bson::String(s) => Value::String(s.clone()),
        bson::Bson::Array(arr) => {
            Value::Array(arr.iter().map(|b| bson_to_json_value(b)).collect())
        }
        bson::Bson::Document(doc) => bson_document_to_json(doc),
        _ => Value::Null,
    }
}
