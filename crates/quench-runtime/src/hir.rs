//! High-level intermediate representation (HIR) and a tiny register-based
//! interpreter used for the shadow-tree fast path.

use std::rc::Rc;

use crate::value::{
    strict_eq, to_bool, to_number, JsError, Value,
};

/// Index of a local register within a call frame.
pub type Local = u16;

/// Opaque id of a basic block within a `HirFunction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockId(pub usize);

/// A constant value referenced by index from `HirValue::Const`.
#[derive(Debug, Clone, PartialEq)]
pub enum HirConst {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
    Undefined,
}

/// A single instruction in a basic block.
#[derive(Debug, Clone, PartialEq)]
pub enum HirValue {
    /// Load a constant into a target local.
    Const { target: Local, id: usize },
    /// Load the current `this` binding into a target local.
    This { target: Local },
    /// Load a global variable by name into a target local.
    LoadGlobal { target: Local, name: String },
    /// Store a local into a global variable.
    StoreGlobal { name: String, source: Local },
    /// Apply a binary operator and store the result.
    Binary { target: Local, op: BinaryOp, left: Local, right: Local },
}

/// Control-flow terminator for a basic block.
#[derive(Debug, Clone, PartialEq)]
pub enum Terminator {
    /// Branch to `then_block` if `cond` is truthy, otherwise `else_block`.
    Branch { cond: Local, then_block: BlockId, else_block: BlockId },
    /// Unconditional jump.
    Jump(BlockId),
    /// Return the value in `local`.
    Return(Local),
    /// Return undefined.
    ReturnUndefined,
}

/// Binary operators supported by the HIR interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    StrictEq,
    Lt,
    And,
    Or,
}

/// A basic block: a sequence of value instructions plus a terminator.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HirBlock {
    pub values: Vec<HirValue>,
    pub terminator: Option<Terminator>,
}

impl HirBlock {
    /// Append a value instruction to the block.
    pub fn push(&mut self, value: HirValue) {
        self.values.push(value);
    }

    /// Set the block terminator.
    pub fn set_terminator(&mut self, term: Terminator) {
        self.terminator = Some(term);
    }
}

/// A function represented as a control-flow graph of basic blocks.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HirFunction {
    pub blocks: Vec<HirBlock>,
    pub locals: usize,
    pub constants: Vec<HirConst>,
}

impl HirFunction {
    /// Create a new empty block and return its id.
    pub fn add_block(&mut self) -> BlockId {
        let id = self.blocks.len();
        self.blocks.push(HirBlock::default());
        BlockId(id)
    }

    /// Mutably borrow a basic block by id.
    pub fn block_mut(&mut self, id: BlockId) -> &mut HirBlock {
        &mut self.blocks[id.0]
    }

    /// Allocate a fresh local register and return its index.
    pub fn alloc_local(&mut self) -> Local {
        let id = self.locals;
        self.locals += 1;
        id as Local
    }

    /// Add a constant and return its index.
    pub fn add_const(&mut self, c: HirConst) -> usize {
        let id = self.constants.len();
        self.constants.push(c);
        id
    }
}

/// A top-level HIR program, consisting of named functions.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HirProgram {
    pub items: Vec<HirItem>,
}

impl HirProgram {
    /// Find a function by name.
    pub fn find_function(&self, name: &str) -> Option<&HirFunction> {
        self.items.iter().find_map(|item| match item {
            HirItem::Function { name: n, func } if n == name => Some(func),
            _ => None,
        })
    }
}

/// An item in a HIR program.
#[derive(Debug, Clone, PartialEq)]
pub enum HirItem {
    Function { name: String, func: HirFunction },
}

/// Run a HIR function to completion and return its result.
pub fn run_hir_function(
    func: Rc<HirFunction>,
    _args: Vec<Value>,
    this: Value,
) -> Result<Value, JsError> {
    let mut locals: Vec<Value> = (0..func.locals).map(|_| Value::Undefined).collect();
    let mut block = BlockId(0);
    let mut pc = 0;

    loop {
        let b = func.blocks.get(block.0).ok_or_else(|| {
            JsError(format!("HIR: invalid block id {}", block.0))
        })?;

        if pc < b.values.len() {
            let instr = &b.values[pc];
            pc += 1;
            match instr {
                HirValue::Const { target, id } => {
                    let c = func.constants.get(*id).ok_or_else(|| {
                        JsError(format!("HIR: invalid constant id {}", id))
                    })?;
                    let v = match c {
                        HirConst::Number(n) => Value::Number(*n),
                        HirConst::String(s) => Value::String(s.clone()),
                        HirConst::Bool(b) => Value::Boolean(*b),
                        HirConst::Null => Value::Null,
                        HirConst::Undefined => Value::Undefined,
                    };
                    locals[usize::from(*target)] = v;
                }
                HirValue::This { target } => {
                    locals[usize::from(*target)] = this.clone();
                }
                HirValue::LoadGlobal { target, name } => {
                    // The HIR fast path does not maintain a real global environment;
                    // unknown globals evaluate to undefined.
                    let v = if name == "undefined" {
                        Value::Undefined
                    } else if name == "Infinity" {
                        Value::Number(f64::INFINITY)
                    } else if name == "NaN" {
                        Value::Number(f64::NAN)
                    } else {
                        Value::Undefined
                    };
                    locals[usize::from(*target)] = v;
                }
                HirValue::StoreGlobal { .. } => {
                    // No global environment in the HIR fast path.
                }
                HirValue::Binary { target, op, left, right } => {
                    let l = locals.get(usize::from(*left)).cloned().unwrap_or(Value::Undefined);
                    let r = locals.get(usize::from(*right)).cloned().unwrap_or(Value::Undefined);
                    let result = eval_binary_op(*op, &l, &r)?;
                    locals[usize::from(*target)] = result;
                }
            }
            continue;
        }

        match b.terminator {
            Some(Terminator::Branch { cond, then_block, else_block }) => {
                let v = locals.get(usize::from(cond)).cloned().unwrap_or(Value::Undefined);
                block = if to_bool(&v) { then_block } else { else_block };
                pc = 0;
            }
            Some(Terminator::Jump(target)) => {
                block = target;
                pc = 0;
            }
            Some(Terminator::Return(local)) => {
                return Ok(locals.get(usize::from(local)).cloned().unwrap_or(Value::Undefined));
            }
            Some(Terminator::ReturnUndefined) => {
                return Ok(Value::Undefined);
            }
            None => {
                return Err(JsError("HIR: block has no terminator".to_string()));
            }
        }
    }
}

fn eval_binary_op(op: BinaryOp, left: &Value, right: &Value) -> Result<Value, JsError> {
    match op {
        BinaryOp::Add => {
            // Simplified: numeric add for the HIR milestone.
            Ok(Value::Number(to_number(left) + to_number(right)))
        }
        BinaryOp::Sub => Ok(Value::Number(to_number(left) - to_number(right))),
        BinaryOp::Mul => Ok(Value::Number(to_number(left) * to_number(right))),
        BinaryOp::Div => Ok(Value::Number(to_number(left) / to_number(right))),
        BinaryOp::Rem => Ok(Value::Number(to_number(left) % to_number(right))),
        BinaryOp::Eq => Ok(Value::Boolean(crate::value::loose_eq(left, right))),
        BinaryOp::StrictEq => Ok(Value::Boolean(strict_eq(left, right))),
        BinaryOp::Lt => Ok(Value::Boolean(to_number(left) < to_number(right))),
        BinaryOp::And => {
            if to_bool(left) {
                Ok(right.clone())
            } else {
                Ok(left.clone())
            }
        }
        BinaryOp::Or => {
            if to_bool(left) {
                Ok(left.clone())
            } else {
                Ok(right.clone())
            }
        }
    }
}
