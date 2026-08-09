//! Residual operations. These are the VM's physical input, not a second AST.

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Const {
        dst: u16,
        value: Constant,
    },
    Binary {
        dst: u16,
        operator: BinaryOp,
        lhs: u16,
        rhs: u16,
    },
    Return {
        src: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Exponentiate,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Number(f64),
    Boolean(bool),
    Null,
    Undefined,
}
