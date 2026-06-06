//! Pattern types
//!

use super::Expr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Pat {
    Ident {
        name: String,
        type_: Option<super::Type>,
        optional: bool,
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

    pub fn binding_names(&self) -> Vec<String> {
        match self {
            Pat::Ident { name, .. } => vec![name.clone()],
            Pat::Array { elems, rest } => self.binding_names_array(elems, rest),
            Pat::Object { props, rest } => self.binding_names_object(props, rest),
            Pat::Assign { left, .. } => left.binding_names(),
            Pat::Rest { arg } => arg.binding_names(),
            Pat::Default { arg, .. } => arg.binding_names(),
        }
    }

    fn binding_names_array(&self, elems: &[Option<Pat>], rest: &Option<Box<Pat>>) -> Vec<String> {
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

    fn binding_names_object(&self, props: &[ObjectPatProp], rest: &Option<Box<Pat>>) -> Vec<String> {
        let mut names = Vec::new();
        for prop in props {
            match prop {
                ObjectPatProp::Init { value, .. } => names.extend(value.binding_names()),
                ObjectPatProp::Rest { arg } | ObjectPatProp::Spread { arg } => names.extend(arg.binding_names()),
                ObjectPatProp::Method { .. } => {}
            }
        }
        if let Some(r) = rest {
            names.extend(r.binding_names());
        }
        names
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
