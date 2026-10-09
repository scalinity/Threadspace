//! An order-preserving JSON value for editing provider configuration (SPEC
//! §19.2). Objects keep document key order and numbers keep their text as
//! written, so an edit leaves unrelated configuration meaningfully unchanged.
//! The workspace's `serde_json::Value` cannot: its map sorts keys, and the
//! `preserve_order`/`arbitrary_precision` features would change that type for
//! every crate, including the canonical hashing ones. Parsing goes through
//! serde_json's own deserializer (each value is first captured as a raw
//! fragment), so the accepted syntax is exactly serde_json's; duplicate keys
//! are refused because the merge could not say which one the provider reads.

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::{self, Serialize, Serializer};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    /// The number's text exactly as the document wrote it.
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(Map),
}

/// Object entries in document order. Keys are unique.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Map(Vec<(String, Value)>);

impl Map {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0
            .iter_mut()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// Replaces an existing key's value in place, or appends the key.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        match self.get_mut(&key) {
            Some(slot) => *slot = value,
            None => self.0.push((key, value)),
        }
    }

    /// Removes a key, keeping the order of the others.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let index = self.0.iter().position(|(name, _)| name == key)?;
        Some(self.0.remove(index).1)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(name, _)| name.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<(String, Value)> for Map {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(entries: I) -> Self {
        let mut map = Self::new();
        for (key, value) in entries {
            map.insert(key, value);
        }
        map
    }
}

impl Value {
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// Two-space indented text with a trailing newline. Fails only for a
    /// `Number` built from text that is not a JSON number.
    pub fn to_pretty(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn string(text: impl Into<String>) -> Self {
        Self::String(text.into())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Self::Object(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Map> {
        match self {
            Self::Object(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object().and_then(|map| map.get(key))
    }

    fn from_fragment(text: &str) -> Result<Self, serde_json::Error> {
        match text.as_bytes().first() {
            Some(b'{') => serde_json::from_str(text).map(Self::Object),
            Some(b'[') => serde_json::from_str(text).map(Self::Array),
            Some(b'"') => serde_json::from_str(text).map(Self::String),
            Some(b't' | b'f') => serde_json::from_str(text).map(Self::Bool),
            Some(b'n') => Ok(Self::Null),
            // The raw capture accepted it, so it is a JSON number.
            _ => Ok(Self::Number(text.to_owned())),
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        Self::from_fragment(raw.get()).map_err(de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for Map {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries;

        impl<'de> Visitor<'de> for Entries {
            type Value = Map;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Map, A::Error> {
                let mut entries: Vec<(String, Value)> = Vec::new();
                while let Some(key) = access.next_key::<String>()? {
                    if entries.iter().any(|(name, _)| *name == key) {
                        return Err(de::Error::custom(format!("duplicate key `{key}`")));
                    }
                    let value = access.next_value()?;
                    entries.push((key, value));
                }
                Ok(Map(entries))
            }
        }

        deserializer.deserialize_map(Entries)
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Number(text) => RawValue::from_string(text.clone())
                .map_err(ser::Error::custom)?
                .serialize(serializer),
            Self::String(text) => serializer.serialize_str(text),
            Self::Array(items) => serializer.collect_seq(items),
            Self::Object(map) => {
                serializer.collect_map(map.0.iter().map(|(key, value)| (key, value)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) -> String {
        let value = Value::parse(text.as_bytes()).expect("parse");
        let printed = value.to_pretty().expect("print");
        assert_eq!(
            Value::parse(&printed).expect("reparse"),
            value,
            "printing is stable"
        );
        String::from_utf8(printed).expect("utf-8")
    }

    #[test]
    fn nested_objects_keep_document_order() {
        let text = "{\n  \"zeta\": {\n    \"b\": [\n      1,\n      {\n        \"y\": true,\n        \"x\": null\n      }\n    ],\n    \"a\": \"s\"\n  },\n  \"alpha\": []\n}\n";
        assert_eq!(round_trip(text), text);
        let value = Value::parse(text.as_bytes()).expect("parse");
        let keys: Vec<&str> = value.as_object().expect("object").keys().collect();
        assert_eq!(keys, ["zeta", "alpha"]);
    }

    #[test]
    fn compact_input_prints_two_space_indent_with_trailing_newline() {
        assert_eq!(
            round_trip(r#"{"b":[1,2],"a":{"c":"d"}}"#),
            "{\n  \"b\": [\n    1,\n    2\n  ],\n  \"a\": {\n    \"c\": \"d\"\n  }\n}\n"
        );
    }

    #[test]
    fn empty_containers_print_inline() {
        assert_eq!(round_trip("{}"), "{}\n");
        assert_eq!(round_trip("[]"), "[]\n");
        assert_eq!(
            round_trip(r#"{"o":{},"a":[]}"#),
            "{\n  \"o\": {},\n  \"a\": []\n}\n"
        );
    }

    #[test]
    fn unicode_escapes_decode_and_reprint() {
        let value = Value::parse(br#"{"k\u00e9y":"caf\u00e9 \ud83d\ude00 \u0001 \"q\" \\"}"#)
            .expect("parse");
        let map = value.as_object().expect("object");
        assert_eq!(
            map.get("kéy").and_then(Value::as_str),
            Some("café 😀 \u{1} \"q\" \\")
        );
        let printed = String::from_utf8(value.to_pretty().expect("print")).expect("utf-8");
        assert_eq!(
            printed,
            "{\n  \"kéy\": \"café 😀 \\u0001 \\\"q\\\" \\\\\"\n}\n"
        );
        assert_eq!(
            round_trip("[\"日本語\", \"ü\"]"),
            "[\n  \"日本語\",\n  \"ü\"\n]\n"
        );
    }

    #[test]
    fn numbers_keep_their_text() {
        let text = "[\n  123456789012345678901234567890,\n  -98765432109876543210,\n  18446744073709551616,\n  1.50,\n  1e3,\n  -0,\n  2.5E-10,\n  0.1\n]\n";
        assert_eq!(round_trip(text), text);
        let value = Value::parse(text.as_bytes()).expect("parse");
        assert_eq!(
            value.as_array().expect("array")[0],
            Value::Number("123456789012345678901234567890".into())
        );
    }

    #[test]
    fn duplicate_keys_are_refused_at_any_depth() {
        assert!(Value::parse(br#"{"a":1,"a":2}"#).is_err());
        assert!(Value::parse(br#"{"x":[{"a":1,"b":2,"a":3}]}"#).is_err());
        assert!(Value::parse(br#"{"a":1,"b":{"a":2}}"#).is_ok());
    }

    #[test]
    fn invalid_documents_are_refused() {
        for text in [
            "",
            "{",
            "{\"a\":}",
            "[1,]",
            "01",
            "NaN",
            "{} {}",
            "\"\\ud800\"",
        ] {
            assert!(Value::parse(text.as_bytes()).is_err(), "{text:?}");
        }
    }

    #[test]
    fn map_edits_keep_order() {
        let mut map: Map = [("a", 1), ("b", 2), ("c", 3)]
            .into_iter()
            .map(|(key, n)| (key.to_owned(), Value::Number(n.to_string())))
            .collect();
        map.insert("b", Value::Null);
        map.insert("d", Value::Bool(true));
        assert_eq!(map.remove("a"), Some(Value::Number("1".into())));
        assert_eq!(map.keys().collect::<Vec<_>>(), ["b", "c", "d"]);
        assert_eq!(map.get("b"), Some(&Value::Null));
    }
}
