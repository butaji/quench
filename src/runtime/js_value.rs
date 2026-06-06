/// Type for JavaScript values (placeholder for actual JS interop)
#[derive(Debug, Clone)]
pub enum JsValue {
    Null,
    Undefined,
    Bool(bool),
    Number(f64),
    String(String),
    Object(Vec<(String, JsValue)>),
    Array(Vec<JsValue>),
}

#[allow(dead_code)]
impl JsValue {
    pub fn null() -> Self {
        JsValue::Null
    }

    pub fn undefined() -> Self {
        JsValue::Undefined
    }

    pub fn string(s: impl Into<String>) -> Self {
        JsValue::String(s.into())
    }

    pub fn number(n: f64) -> Self {
        JsValue::Number(n)
    }

    pub fn bool(b: bool) -> Self {
        JsValue::Bool(b)
    }

    pub fn object(props: Vec<(String, JsValue)>) -> Self {
        JsValue::Object(props)
    }

    pub fn array(values: Vec<JsValue>) -> Self {
        JsValue::Array(values)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, JsValue::Null)
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self, JsValue::Undefined)
    }

    pub fn as_string(&self) -> Option<&String> {
        match self {
            JsValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            JsValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl Default for JsValue {
    fn default() -> Self {
        JsValue::Undefined
    }
}

impl From<String> for JsValue {
    fn from(s: String) -> Self {
        JsValue::String(s)
    }
}

impl From<&str> for JsValue {
    fn from(s: &str) -> Self {
        JsValue::String(s.to_string())
    }
}

impl From<bool> for JsValue {
    fn from(b: bool) -> Self {
        JsValue::Bool(b)
    }
}

impl From<i32> for JsValue {
    fn from(n: i32) -> Self {
        JsValue::Number(n as f64)
    }
}

impl From<f64> for JsValue {
    fn from(n: f64) -> Self {
        JsValue::Number(n)
    }
}

/// Placeholder for web_sys types in server context
#[allow(dead_code)]
pub mod web_sys {
    pub struct Event;
    pub struct MouseEvent;
    pub struct InputEvent;
    pub struct KeyboardEvent;
    pub struct FocusEvent;
    pub struct SubmitEvent;
    pub struct ChangeEvent;
}
