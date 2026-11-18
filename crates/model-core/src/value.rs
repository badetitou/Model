use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Open-ended facts a frontend or an analysis attaches to an entity
/// (annotations, modifiers, visibility, HTTP endpoints...). Keys are
/// namespaced by convention: `java.annotations`, `http.endpoints`.
pub type Props = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_string())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::List(v.into_iter().map(Into::into).collect())
    }
}
