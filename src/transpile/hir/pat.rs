//! Pattern types

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
}
