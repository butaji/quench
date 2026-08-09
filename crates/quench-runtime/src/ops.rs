//! Residual operations. These are the VM's physical input, not a second AST.

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Const { dst: u16, value: Constant },
    Return { src: u16 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Number(f64),
    Boolean(bool),
    Null,
    Undefined,
}
