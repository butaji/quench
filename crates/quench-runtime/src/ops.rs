//! Residual operations. These are the VM's physical input, not a second AST.

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Const {
        dst: u16,
        value: Constant,
    },
    StoreLocal {
        slot: u16,
        src: u16,
    },
    LoadLocal {
        dst: u16,
        slot: u16,
    },
    MakeArray {
        dst: u16,
        elements: Vec<u16>,
    },
    MakeObject {
        dst: u16,
        properties: Vec<(String, u16)>,
    },
    MakeBuiltin {
        dst: u16,
        builtin: Builtin,
    },
    GetProperty {
        dst: u16,
        object: u16,
        key: String,
    },
    MakeFunction {
        dst: u16,
        body: Vec<Op>,
        params: u16,
    },
    Call {
        dst: u16,
        callee: u16,
        args: Vec<u16>,
    },
    Unary {
        dst: u16,
        operator: UnaryOp,
        src: u16,
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
pub enum Builtin {
    Eval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Exponentiate,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    LessThan,
    LessEqual,
    GreaterThan,
    GreaterEqual,
    BitwiseOr,
    BitwiseXor,
    BitwiseAnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Plus,
    Minus,
    Not,
    Void,
    Typeof,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Number(f64),
    Boolean(bool),
    String(String),
    Null,
    Undefined,
}
