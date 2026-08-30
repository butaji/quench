//! QuickJS stack bytecode. Compact ops; max stack is compile-time.

/// Stack effect is data, not a comment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StackUse {
    pub pop: u8,
    pub push: u8,
}

/// Subset of `quickjs-opcode.h`. The interpreter is a table over this enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Op {
    PushI32,
    PushConst,
    PushAtom,
    Drop,
    Dup,
    Nip,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    GetField,
    PutField,
    GetVarRef,
    PutVarRef,
    Call,
    TailCall,
    Return,
    ReturnUndef,
    IfFalse,
    IfTrue,
    Goto,
    Throw,
    ToObject,
    ToPropkey,
    DefineField,
    SetName,
    CheckDefine,
}

impl Op {
    pub fn stack(self) -> StackUse {
        match self {
            Self::PushI32 | Self::PushConst | Self::PushAtom | Self::ReturnUndef => {
                StackUse { pop: 0, push: 1 }
            }
            Self::Drop | Self::Return | Self::Throw | Self::IfFalse | Self::IfTrue => {
                StackUse { pop: 1, push: 0 }
            }
            Self::Dup => StackUse { pop: 0, push: 1 },
            Self::Nip | Self::Goto | Self::SetName | Self::CheckDefine => {
                StackUse { pop: 0, push: 0 }
            }
            Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Mod
            | Self::Shl
            | Self::PutField
            | Self::PutVarRef
            | Self::DefineField => StackUse { pop: 2, push: 1 },
            Self::GetField | Self::GetVarRef | Self::ToObject | Self::ToPropkey => {
                StackUse { pop: 1, push: 1 }
            }
            Self::Call | Self::TailCall => StackUse { pop: 1, push: 1 },
        }
    }
}

/// One compiled function. `stack_size` is known at compile time (QuickJS).
#[derive(Clone, Debug, PartialEq)]
pub struct Bytecode {
    pub ops: Box<[Op]>,
    pub stack_size: u16,
    pub arg_count: u16,
    pub var_count: u16,
}

impl Bytecode {
    pub fn from_ops(ops: Box<[Op]>, arg_count: u16, var_count: u16) -> Self {
        let stack_size = max_stack(&ops);
        Self {
            ops,
            stack_size,
            arg_count,
            var_count,
        }
    }
}

fn max_stack(ops: &[Op]) -> u16 {
    let mut depth = 0i32;
    let mut max = 0i32;
    for op in ops {
        let use_ = op.stack();
        depth -= use_.pop as i32;
        depth += use_.push as i32;
        if depth > max {
            max = depth;
        }
    }
    max.max(0) as u16
}

#[cfg(test)]
mod tests {
    use super::{Bytecode, Op};

    #[test]
    fn compile_time_stack_size() {
        let bc = Bytecode::from_ops(
            Box::new([Op::PushI32, Op::PushI32, Op::Add, Op::Return]),
            0,
            0,
        );
        assert_eq!(bc.stack_size, 2);
    }
}
