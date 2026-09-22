use crate::Value;
use std::fmt;

#[derive(Debug)]
pub struct JsError(pub(crate) ErrorMessage);

#[derive(Debug)]
pub(crate) struct ErrorMessage {
    payload: Box<ErrorPayload>,
}

#[derive(Debug)]
struct ErrorPayload {
    text: String,
    thrown: Option<Value>,
}

impl From<&str> for ErrorMessage {
    fn from(value: &str) -> Self {
        Self {
            payload: Box::new(ErrorPayload {
                text: value.into(),
                thrown: None,
            }),
        }
    }
}

impl From<String> for ErrorMessage {
    fn from(value: String) -> Self {
        Self {
            payload: Box::new(ErrorPayload {
                text: value,
                thrown: None,
            }),
        }
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.payload.text)
    }
}

impl JsError {
    pub(crate) fn thrown(value: Value, message: String) -> Self {
        Self(ErrorMessage {
            payload: Box::new(ErrorPayload {
                text: message,
                thrown: Some(value),
            }),
        })
    }

    pub(crate) fn thrown_value(&self) -> Option<Value> {
        self.0.payload.thrown
    }

    pub(crate) fn validation(message: String) -> Self {
        Self(ErrorMessage::from(format!(
            "invalid residual program: {message}"
        )))
    }

    pub(super) fn into_message(self) -> String {
        self.0.payload.text.clone()
    }
}
