//! Pattern types
//!
//! allow:complexity,too_many_lines

use super::Expr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Pat {
    Ident {
        name: String,
        type_: Option<super::Type>,
    },
    Array {
        elems: Vec<Option<Pat>>,
        rest: Option<Box<Pat>>,
    },
    Object {
        props: Vec<ObjectPatProp>,
        rest: Option<Box<Pat>>,
    },
    Assign {
        left: Box<Pat>,
        right: Box<Expr>,
    },
    Rest {
        arg: Box<Pat>,
    },
    Default {
        arg: Box<Pat>,
        default: Box<Expr>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ObjectPatProp {
    Init { key: String, value: Pat },
    Rest { arg: Box<Pat> },
    /// Spread pattern in object destructuring: { ...rest }
    Spread { arg: Box<Pat> },
    /// Method definition in object pattern: { key() {} }
    Method { key: String },
}

impl Pat {
    /// Check if this pattern is a simple identifier
    pub fn is_ident(&self) -> bool {
        matches!(self, Pat::Ident { .. })
    }

    /// Get the name if this is an identifier pattern
    pub fn as_ident(&self) -> Option<&str> {
        if let Pat::Ident { name, .. } = self {
            Some(name)
        } else {
            None
        }
    }

    // allow:complexity,too_many_lines
    pub fn binding_names(&self) -> Vec<String> {
        match self {
            Pat::Ident { name, .. } => vec![name.clone()],
            Pat::Array { elems, rest } => {
                let mut names = Vec::new();
                for elem in elems {
                    if let Some(p) = elem {
                        names.extend(p.binding_names());
                    }
                }
                if let Some(r) = rest {
                    names.extend(r.binding_names());
                }
                names
            }
            Pat::Object { props, rest } => {
                let mut names = Vec::new();
                for prop in props {
                    match prop {
                        ObjectPatProp::Init { value, .. } => names.extend(value.binding_names()),
                        ObjectPatProp::Rest { arg } => names.extend(arg.binding_names()),
                        ObjectPatProp::Spread { arg } => names.extend(arg.binding_names()),
                        ObjectPatProp::Method { .. } => {}
                    }
                }
                if let Some(r) = rest {
                    names.extend(r.binding_names());
                }
                names
            }
            Pat::Assign { left, .. } => left.binding_names(),
            Pat::Rest { arg } => arg.binding_names(),
            Pat::Default { arg, .. } => arg.binding_names(),
        }
    }
}

impl ObjectPatProp {
    /// Get the key name if this prop has one
    pub fn key_name(&self) -> Option<&str> {
        match self {
            ObjectPatProp::Init { key, .. } => Some(key),
            ObjectPatProp::Method { key, .. } => Some(key),
            ObjectPatProp::Rest { .. } | ObjectPatProp::Spread { .. } => None,
        }
    }
}
