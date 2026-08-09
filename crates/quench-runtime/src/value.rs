//! Machine-sized runtime values for the residual kernel.

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Boolean(bool),
    String(String),
    Null,
    Undefined,
}
