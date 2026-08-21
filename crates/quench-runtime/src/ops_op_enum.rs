#[derive(Debug, Clone, PartialEq)]
pub enum ArrayElement {
    Value(u16),
    Elision,
    Spread(u16),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstanceFieldKeyOp {
    Static(String),
    Dynamic(u16),
    Private(crate::facts::PrivateNameId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstanceFieldInitializerOp {
    pub body: crate::machine::FunctionCode,
    pub captures: u16,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateAccessorOp {
    pub get: Option<u16>,
    pub set: Option<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppendInstanceFieldOp {
    pub constructor: u16,
    pub key: InstanceFieldKeyOp,
    pub initializer: Option<InstanceFieldInitializerOp>,
    pub is_static: bool,
    /// Register holding the element value directly (private methods), bypassing
    /// an initializer executable.
    pub value: Option<u16>,
    /// Accessor functions for a private accessor element.
    pub accessor: Option<PrivateAccessorOp>,
}

include!("ops_op_enum_tail.rs");
