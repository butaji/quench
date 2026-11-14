//! A number loop's plan for one assignment of entry kinds: every register's slot kind,
//! inferred over the body to a fixpoint, and the body as expression trees.
//!
//! A register the body defines once and reads once, later in the body and nowhere else,
//! is a temporary: its defining tree becomes an operand of its reader, so the value flows
//! in machine registers. A reader takes only temporaries defined right before it, in the
//! order it evaluates them, so a fused statement still runs its instructions in PC order
//! and a leave inside it resumes exactly.

use super::*;

/// An array key.
#[derive(Clone, Copy, Debug)]
pub(super) enum KeyForm {
    Int(u16),
    Double(u16),
    Constant(usize),
    /// An int32 register stepped by one first (`x[++i]`), the step's own PC its leave.
    Step { register: u16, decrement: bool, at: u32 },
}

/// One instruction's computation, with the operands it reads folded in.
pub(super) enum Tree {
    Double(u16),
    Int(u16),
    Constant(f64),
    Read { dst: u16, array: u8, key: KeyForm, at: u32 },
    Arith { op: u8, dst: u16, left: Box<Tree>, right: Box<Tree> },
    Numeric { operator: BinaryOperator, dst: u16, left: Box<Tree>, right: Box<Tree> },
    Bits { op: u8, dst: u16, left: Box<Tree>, right: Box<Tree> },
    Move { dst: u16, int: bool, src: Box<Tree> },
    Step { dst: u16, src: u16, int: bool, decrement: bool, at: u32 },
}

impl Tree {
    /// Whether the tree's value is an int32.
    pub(super) fn is_int(&self) -> bool {
        match self {
            Tree::Int(_) | Tree::Bits { .. } => true,
            Tree::Move { int, .. } | Tree::Step { int, .. } => *int,
            Tree::Constant(number) => i32_of(*number).is_some(),
            _ => false,
        }
    }
}

/// The int32 a constant is exactly, `-0` excluded.
pub(super) fn i32_of(number: f64) -> Option<i32> {
    Value::number(number).as_int()
}

/// A body statement.
pub(super) enum Statement {
    Tree(Tree),
    Write { array: u8, key: KeyForm, value: Tree, int: bool, at: u32 },
}

/// The slot kinds of a loop: entry kinds joined with what each step defines, to a fixpoint.
pub(super) fn infer(found: &NumberLoop, entry: &[(u16, Kind)]) -> Option<[Option<Kind>; SLOTS]> {
    let mut kinds: [Option<Kind>; SLOTS] = [None; SLOTS];
    for &(register, kind) in entry {
        kinds[usize::from(register)] = Some(kind);
    }
    let counter = kinds[usize::from(found.counter.1)].filter(|&kind| kind != Kind::Array)?;
    loop {
        let mut changed = join(&mut kinds, found.counter.0, counter)?;
        for (_, instruction) in found.body.iter() {
            if let Some((dst, kind)) = defines(&kinds, *instruction) {
                changed |= join(&mut kinds, dst, kind)?;
            }
        }
        if !changed {
            return Some(kinds);
        }
    }
}

/// The register a step defines and the kind it gives it.
fn defines(kinds: &[Option<Kind>; SLOTS], instruction: Instruction) -> Option<(u16, Kind)> {
    match instruction {
        Instruction::Read { dst, .. } => Some((dst, Kind::Double)),
        Instruction::Binary { dst, operator, .. } => {
            let int = crate::vm::operations::int32_bitwise_kind(operator) == Some(true);
            Some((dst, if int { Kind::Int } else { Kind::Double }))
        }
        Instruction::Move { dst, src } => Some((dst, operand_kind(kinds, src).unwrap_or(Kind::Double))),
        Instruction::Step { dst, src, .. } => Some((dst, kinds[usize::from(src)].unwrap_or(Kind::Double))),
        Instruction::Write { .. } => None,
    }
}

fn operand_kind(kinds: &[Option<Kind>; SLOTS], operand: Operand) -> Option<Kind> {
    match operand {
        Operand::Register(register) => kinds[usize::from(register)],
        Operand::Constant(value) => Some(if value.as_int().is_some() { Kind::Int } else { Kind::Double }),
    }
}

/// Joins `kind` into a register's: int32 and double join to double; an array joins with
/// nothing, so a body that redefines an array or computes with one has no plan.
fn join(kinds: &mut [Option<Kind>; SLOTS], register: u16, kind: Kind) -> Option<bool> {
    let slot = &mut kinds[usize::from(register)];
    let joined = match (*slot, kind) {
        (None, kind) => kind,
        (Some(Kind::Array), _) | (_, Kind::Array) => return None,
        (Some(Kind::Int), Kind::Int) => Kind::Int,
        _ => Kind::Double,
    };
    let changed = *slot != Some(joined);
    *slot = Some(joined);
    Some(changed)
}

/// Builds the statements of a body under `kinds`, fusing temporaries into their readers.
pub(super) struct Builder<'a> {
    pub(super) found: &'a NumberLoop,
    pub(super) kinds: &'a [Option<Kind>; SLOTS],
    /// The array registers by the index the nodes name them with, and whether a step
    /// writes each.
    pub(super) arrays: Vec<(u16, bool)>,
    /// Trees of temporaries whose reader has not come yet, in PC order.
    pending: Vec<(u16, Tree)>,
    pub(super) statements: Vec<Statement>,
}

impl<'a> Builder<'a> {
    pub(super) fn new(found: &'a NumberLoop, kinds: &'a [Option<Kind>; SLOTS]) -> Self {
        Builder { found, kinds, arrays: Vec::new(), pending: Vec::new(), statements: Vec::new() }
    }

    /// Every step, in order; `None` where a step needs what its kinds cannot give.
    pub(super) fn build(mut self) -> Option<Self> {
        let body = &self.found.body;
        let mut index = 0;
        while index < body.len() {
            let (at, instruction) = body[index];
            match (instruction, body.get(index + 1)) {
                // `x[++i]`: the update folds into the read's key.
                (Instruction::Step { dst, src, decrement }, Some(&(read_at, Instruction::Read { dst: read, base, key: Operand::Register(key) })))
                    if dst == src && key == dst && self.kinds[usize::from(dst)] == Some(Kind::Int) =>
                {
                    self.read(read_at, read, base, KeyForm::Step { register: dst, decrement, at })?;
                    index += 2;
                }
                _ => {
                    self.step(at, instruction)?;
                    index += 1;
                }
            }
        }
        self.flush();
        Some(self)
    }

    /// A read whose key form the caller chose; nothing folds into a key.
    fn read(&mut self, at: u32, dst: u16, base: u16, key: KeyForm) -> Option<()> {
        if self.temporary(base) || matches!(key, KeyForm::Int(register) | KeyForm::Double(register) if self.temporary(register)) {
            self.flush();
        }
        let tree = Tree::Read { dst, array: self.array(base, false)?, key, at };
        self.define(dst, tree);
        Some(())
    }

    /// A tree defining `dst`: pending when `dst` is a temporary, else its own statement.
    fn define(&mut self, dst: u16, tree: Tree) {
        if self.reads_of(dst) == 1 && self.defs_of(dst) == 1 && dst != self.found.counter.0 {
            self.pending.push((dst, tree));
        } else {
            self.flush();
            self.statements.push(Statement::Tree(tree));
        }
    }

    fn step(&mut self, at: u32, instruction: Instruction) -> Option<()> {
        // The temporaries this step folds in, in the order it evaluates them, must be the
        // last pending ones in PC order; a pending key or base becomes its own statement.
        let fused: Vec<u16> = instruction.operands().filter(|&register| self.temporary(register)).collect();
        let others = instruction.reads().filter(|&register| self.temporary(register)).count() > fused.len();
        let suffix = self.pending.len().checked_sub(fused.len()).filter(|&start| {
            self.pending[start..].iter().map(|(register, _)| *register).eq(fused.iter().copied())
        });
        if suffix.is_none() || others {
            self.flush();
        }
        let tree = match instruction {
            Instruction::Write { base, key, value } => {
                let value = self.operand(Operand::Register(value));
                let int = value.is_int();
                let (array, key) = (self.array(base, true)?, self.key(key)?);
                self.flush();
                self.statements.push(Statement::Write { array, key, value, int, at });
                return Some(());
            }
            Instruction::Read { dst, base, key } => Tree::Read { dst, array: self.array(base, false)?, key: self.key(key)?, at },
            Instruction::Move { dst, src } => {
                let int = self.kinds[usize::from(dst)]? == Kind::Int;
                Tree::Move { dst, int, src: Box::new(self.operand(src)) }
            }
            Instruction::Binary { dst, operator, left, right } => {
                let (left, right) = (Box::new(self.operand(left)), Box::new(self.operand(right)));
                binary_tree(dst, self.kinds[usize::from(dst)]?, operator, left, right)
            }
            Instruction::Step { dst, src, decrement } => {
                let int = self.kinds[usize::from(dst)]? == Kind::Int;
                Tree::Step { dst, src, int, decrement, at }
            }
        };
        self.define(instruction.defined()?, tree);
        Some(())
    }

    /// Whether `register` is a pending temporary.
    fn temporary(&self, register: u16) -> bool {
        self.pending.iter().any(|(pending, _)| *pending == register)
    }

    /// The operand as a tree: a pending temporary's own tree, else its slot or constant.
    fn operand(&mut self, operand: Operand) -> Tree {
        match operand {
            Operand::Constant(value) => Tree::Constant(value.as_number().unwrap_or(f64::NAN)),
            Operand::Register(register) => match self.pending.iter().position(|(pending, _)| *pending == register) {
                Some(index) => self.pending.remove(index).1,
                None if self.kinds[usize::from(register)] == Some(Kind::Int) => Tree::Int(register),
                None => Tree::Double(register),
            },
        }
    }

    /// Emits the pending temporaries as statements, in PC order.
    fn flush(&mut self) {
        for (_, tree) in self.pending.drain(..) {
            self.statements.push(Statement::Tree(tree));
        }
    }

    fn key(&self, key: Operand) -> Option<KeyForm> {
        match key {
            Operand::Constant(value) => usize::try_from(value.as_int()?).ok().map(KeyForm::Constant),
            Operand::Register(register) => match self.kinds[usize::from(register)]? {
                Kind::Int => Some(KeyForm::Int(register)),
                Kind::Double => Some(KeyForm::Double(register)),
                Kind::Array => None,
            },
        }
    }

    fn array(&mut self, register: u16, writes: bool) -> Option<u8> {
        if self.kinds[usize::from(register)] != Some(Kind::Array) {
            return None;
        }
        let index = match self.arrays.iter().position(|&(known, _)| known == register) {
            Some(index) => index,
            None => {
                self.arrays.push((register, false));
                self.arrays.len() - 1
            }
        };
        self.arrays[index].1 |= writes;
        u8::try_from(index).ok().filter(|&index| usize::from(index) < ARRAYS)
    }

    /// How often the loop reads `register`: its body, its test and its update.
    fn reads_of(&self, register: u16) -> usize {
        let found = self.found;
        let test = [found.test_operands.0, found.test_operands.1].into_iter().filter(|operand| operand.register() == Some(register));
        let body = found.body.iter().flat_map(|(_, instruction)| instruction.reads()).filter(|&read| read == register);
        test.count() + body.count() + usize::from(found.counter.1 == register)
    }

    fn defs_of(&self, register: u16) -> usize {
        self.found.body.iter().filter(|(_, instruction)| instruction.defined() == Some(register)).count()
    }
}

/// The tree of `dst = left operator right` under `dst`'s kind.
fn binary_tree(dst: u16, kind: Kind, operator: BinaryOperator, left: Box<Tree>, right: Box<Tree>) -> Tree {
    if kind == Kind::Int {
        let op = match operator {
            BinaryOperator::BitwiseOR => ints::OR,
            BinaryOperator::BitwiseAnd => ints::AND,
            BinaryOperator::BitwiseXOR => ints::XOR,
            BinaryOperator::ShiftLeft => ints::SHL,
            _ => ints::SAR,
        };
        return Tree::Bits { op, dst, left, right };
    }
    let op = match operator {
        BinaryOperator::Addition => nodes::ADD,
        BinaryOperator::Subtraction => nodes::SUB,
        BinaryOperator::Multiplication => nodes::MUL,
        BinaryOperator::Division => nodes::DIV,
        operator => return Tree::Numeric { operator, dst, left, right },
    };
    Tree::Arith { op, dst, left, right }
}
