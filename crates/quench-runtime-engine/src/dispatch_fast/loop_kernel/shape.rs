//! A loop kernel's shape: the loop's body as a dataflow graph over columns, found by two
//! symbolic passes over the body (`Shape::of`).
use quench_runtime::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};
use super::super::*;
use quench_runtime::bytecode::OperandKind;

/// Most operations a kernel keeps (columns of scratch).
const MAX_NODES: usize = 96;
/// Most recurrences, scalars and affine values a kernel keeps.
const MAX_SLOTS: usize = 32;

pub(super) use quench_runtime::vm::engine_types::Arg;

/// `register + offset + step * k` at iteration `k`, `register` an induction (with its step)
/// or an unwritten register (step 0); the register holds an int32 at entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Affine {
    pub(super) register: u16,
    pub(super) offset: i32,
    pub(super) step: Stride,
}

/// An induction's step: a constant, or an unwritten register's int32 value at entry, maybe
/// negated (`i -= height`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Stride {
    Const(i32),
    Register(u16, bool),
}

impl Stride {
    /// The constant step, if it is one.
    pub(super) fn constant(self) -> Option<i32> {
        match self {
            Stride::Const(step) => Some(step),
            Stride::Register(..) => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum NodeOp {
    Arith(NumberArithmetic),
    Bits(NumberBitwise),
    /// `bases[base][affines[index]]`, decoded.
    Load { base: u8, index: u8 },
    /// An affine operand of arithmetic, as a column.
    Iota { index: u8 },
    /// A relation of two Numbers as 1 (holds) or 0: a branch's condition in the body, which
    /// the body's predicated writes read (`Graph::pass`).
    Cmp(NumberRelation),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Node {
    pub(super) op: NodeOp,
    pub(super) a: Arg,
    pub(super) b: Arg,
    /// Reads a recurrence, directly or through another node: runs in the scalar pass.
    pub(super) seq: bool,
    /// Reads only scalars: computed once at entry as a scalar, never as a column.
    pub(super) hoisted: bool,
    /// No recurrence, store or final value reads it: never computed (a branch's untaken
    /// predicate, a select's unused half).
    pub(super) dead: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Store {
    pub(super) base: u8,
    pub(super) index: u8,
    pub(super) value: Arg,
    /// Its position among the body's accesses.
    pub(super) order: u16,
}

/// Where a scalar comes from.
#[derive(Clone, Copy, Debug)]
pub(super) enum Scalar {
    Const(f64),
    Register(u16),
    /// A captured binding, read once at entry (`Sym::Capture`).
    Capture(u32),
    /// `register.length` when the atom is `length` and the register an array or a string,
    /// whose length no kernel body changes; any other field declines the run.
    Length { register: u16, atom: u32 },
    /// Loop-invariant arithmetic over earlier scalars, computed once at entry.
    Computed { op: NodeOp, a: u8, b: u8 },
}

/// A loop kernel's shape (module documentation).
pub(crate) struct Shape {
    /// `affines[counter] REL scalars[bound]` is the loop's test.
    pub(super) counter: u8,
    pub(super) relation: NumberRelation,
    pub(super) bound: u8,
    pub(super) scalars: Vec<Scalar>,
    pub(super) affines: Vec<Affine>,
    pub(super) bases: Vec<u16>,
    pub(super) nodes: Vec<Node>,
    /// Which nodes depend on a recurrence, in body order: the scalar pass.
    pub(super) seq: Vec<u16>,
    pub(super) stores: Vec<Store>,
    /// Loads (node, access order).
    pub(super) loads: Vec<(u16, u16)>,
    /// Recurrence registers and their values at an iteration's end.
    pub(super) phis: Vec<(u16, Arg)>,
    /// Inductions and their steps.
    pub(super) inductions: Vec<(u16, Stride)>,
    /// The other registers the body writes, with their values at an iteration's end.
    pub(super) temps: Vec<(u16, Arg)>,
    /// Registers the body writes on some paths only (under a branch) and no path past it reads:
    /// they keep no final value, so the kernel holds only where they are dead past the loop.
    pub(super) guarded: Vec<u16>,
}

impl Shape {
    /// The constant scalar `slot` holds, if it holds a constant.
    pub(super) fn scalar_constant(&self, slot: u8) -> Option<f64> {
        match self.scalars[usize::from(slot)] {
            Scalar::Const(value) => Some(value),
            _ => None,
        }
    }
}

impl Scalar {
    /// Whether two entry reads read the same value.
    fn same_read(self, other: Self) -> bool {
        match (self, other) {
            (Self::Capture(a), Self::Capture(b)) => a == b,
            (Self::Length { register: a, atom: x }, Self::Length { register: b, atom: y }) => a == b && x == y,
            _ => false,
        }
    }
}

/// A value during the symbolic passes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sym {
    Node(u16),
    Phi(u16),
    Const(f64),
    Affine(u16, i32),
    /// An induction stepped by an unwritten register (`Stride::Register`).
    Strided(u16, u16, bool),
    /// A captured binding's value (a `LoadCapture` immediate): the body runs no code that
    /// could assign it.
    Capture(u32),
    /// A field of an unwritten register, by atom: read once at entry, which holds only for an
    /// array's or a string's `length` (`Scalar::Length`).
    Field(u16, u32),
    /// A register written on one path of the body only, read past that path: its value there is
    /// the register's on the other path, which no column holds.
    Poison,
}

/// A symbolic pass's operation, before lowering.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SymOp {
    Arith(NumberArithmetic),
    Bits(NumberBitwise),
    Load { base: u16, index: (u16, i32) },
    Iota { index: (u16, i32) },
    Cmp(NumberRelation),
}

/// One symbolic pass's graph.
struct Graph<'a> {
    constants: &'a [Value],
    /// Inductions found so far (every register in the first pass).
    induction: &'a [bool],
    written: &'a [bool],
    nodes: Vec<(SymOp, Sym, Sym)>,
    stores: Vec<(u16, (u16, i32), Sym, u16)>,
    loads: Vec<(u16, u16)>,
    accesses: u16,
}

impl<'a> Graph<'a> {
    fn new(constants: &'a [Value], induction: &'a [bool], written: &'a [bool]) -> Self {
        Self { constants, induction, written, nodes: Vec::new(), stores: Vec::new(), loads: Vec::new(), accesses: 0 }
    }

    fn node(&mut self, op: SymOp, a: Sym, b: Sym) -> Option<Sym> {
        // An operator of the same operands computed before is the same column (loads, which
        // stores order, are kept apart).
        if !matches!(op, SymOp::Load { .. }) {
            if let Some(id) = self.nodes.iter().position(|&node| node == (op, a, b)) {
                return Some(Sym::Node(id as u16));
            }
        }
        lower::traced((self.nodes.len() < MAX_NODES).then_some(()), "more nodes")?;
        self.nodes.push((op, a, b));
        Some(Sym::Node((self.nodes.len() - 1) as u16))
    }

    fn operand(&self, file: &[Sym], operand: quench_runtime::bytecode::Operand) -> Option<Sym> {
        match operand.kind()? {
            OperandKind::Register => file.get(usize::from(operand.payload())).copied(),
            OperandKind::Constant => self.constants.get(usize::from(operand.payload()))?.as_number().map(Sym::Const),
            _ => None,
        }
    }

    /// An arithmetic operand: an induction's or offset register's value becomes a column.
    fn arith_operand(&mut self, sym: Sym) -> Option<Sym> {
        match sym {
            Sym::Affine(register, offset) if self.induction[usize::from(register)] || offset != 0 => {
                self.node(SymOp::Iota { index: (register, offset) }, sym, sym)
            }
            other => Some(other),
        }
    }

    /// A base register: unwritten, with no offset.
    fn base(&self, sym: Sym) -> Option<u16> {
        match sym {
            Sym::Affine(register, 0) if !self.written[usize::from(register)] => Some(register),
            _ => None,
        }
    }

    /// A written register plus or minus an unwritten one (`i -= height`), which only an
    /// induction's step may be.
    fn strided(&self, kind: SymOp, a: Sym, b: Sym) -> Option<Sym> {
        let (written, other, negative) = match (kind, a, b) {
            (SymOp::Arith(NumberArithmetic::Add), Sym::Affine(r, 0), Sym::Affine(h, 0))
            | (SymOp::Arith(NumberArithmetic::Add), Sym::Affine(h, 0), Sym::Affine(r, 0))
                if self.written[usize::from(r)] && !self.written[usize::from(h)] =>
            {
                (r, h, false)
            }
            (SymOp::Arith(NumberArithmetic::Subtract), Sym::Affine(r, 0), Sym::Affine(h, 0)) => (r, h, true),
            _ => return None,
        };
        (self.written[usize::from(written)] && !self.written[usize::from(other)]).then_some(Sym::Strided(written, other, negative))
    }

    fn index(sym: Sym) -> Option<(u16, i32)> {
        match sym {
            Sym::Affine(register, offset) => Some((register, offset)),
            _ => None,
        }
    }

    fn access(&mut self) -> u16 {
        self.accesses += 1;
        self.accesses - 1
    }

    /// `source ± 1` (`IncDec`, `Number(IncDec)` of the staged row).
    fn inc_dec(&mut self, source: Register, decrement: bool, file: &[Sym]) -> Option<Sym> {
        let step = if decrement { -1 } else { 1 };
        Some(match *file.get(usize::from(source))? {
            Sym::Affine(register, offset) => Sym::Affine(register, offset.checked_add(step)?),
            Sym::Const(value) => Sym::Const(value + f64::from(step)),
            other => self.node(SymOp::Arith(NumberArithmetic::Add), other, Sym::Const(f64::from(step)))?,
        })
    }

    fn binary(&mut self, instruction: WideInstruction, file: &[Sym]) -> Option<Sym> {
        let (op, operator) = (instruction.op(), instruction.binary_operator());
        let kind = match NumberArithmetic::from_js_instruction(op, operator) {
            Some(arith) => SymOp::Arith(arith),
            None => SymOp::Bits(NumberBitwise::from_js_instruction(op, operator)?),
        };
        let a = self.operand(file, instruction.operand_b())?;
        let b = self.operand(file, instruction.operand_c())?;
        // An induction stepped by a constant (`i += 4`) stays affine, as `inc_dec` steps it.
        if let Some(affine) = shifted(kind, a, b).filter(|&sym| matches!(sym, Sym::Affine(r, _) if self.written[usize::from(r)])) {
            return Some(affine);
        }
        if let Some(strided) = self.strided(kind, a, b) {
            return Some(strided);
        }
        let (a, b) = (self.arith_operand(a)?, self.arith_operand(b)?);
        self.node(kind, a, b)
    }

    /// A named field of a register the loop does not write.
    fn field(&self, instruction: WideInstruction) -> Option<Sym> {
        let quench_runtime::bytecode::FieldLookup::Atom { atom, base, .. } = instruction.field_lookup()? else { return None };
        let register = base.register_index()?;
        (!*self.written.get(usize::from(register))?).then_some(Sym::Field(register, atom))
    }

    /// `base[key]` (`ElementRead` of the staged row).
    fn get_index(&mut self, base: Register, key: quench_runtime::bytecode::Operand, file: &[Sym]) -> Option<Sym> {
        let base = self.base(*file.get(usize::from(base))?)?;
        let index = Self::index(self.operand(file, key)?)?;
        // A second read of an element no store came between reads the first's column.
        let same = |&&(id, order): &&(u16, u16)| {
            matches!(self.nodes[usize::from(id)].0, SymOp::Load { base: b, index: i } if b == base && i == index)
                && self.stores.iter().all(|store| store.3 < order)
        };
        if let Some(&(id, _)) = self.loads.iter().find(same) {
            return Some(Sym::Node(id));
        }
        let order = self.access();
        let node = self.node(SymOp::Load { base, index }, Sym::Const(0.0), Sym::Const(0.0))?;
        let Sym::Node(id) = node else { return None };
        self.loads.push((id, order));
        Some(node)
    }

    /// `base[key] = stored` (`ElementWrite` of the staged row).
    fn set_index(&mut self, stored: Register, base: Register, key: Register, file: &[Sym]) -> Option<()> {
        let value = self.arith_operand(*file.get(usize::from(stored))?)?;
        let base = self.base(*file.get(usize::from(base))?)?;
        let index = Self::index(*file.get(usize::from(key))?)?;
        let order = self.access();
        self.stores.push((base, index, value, order));
        Some(())
    }

    /// The value `instruction` writes to its result register, or `Ok(None)` for one that
    /// writes none; `Err` for an instruction a kernel does not run.
    fn step(&mut self, instruction: WideInstruction, file: &[Sym]) -> Result<Option<Sym>, ()> {
        if writes_local(instruction) {
            return Err(());
        }
        // The staged row decides what the instruction means where it has one; the opcode arms
        // below are the operations the staged decoder does not yet express (`LoadConst` needs the
        // tier's typed constants, `Binary` decodes register operands only, `NumericAdd` and
        // `NumericMultiply` have no staged decoder, `Nop` decodes to nothing).
        let value = match staged_operation(instruction) {
            Some(quench_runtime::staged::Operation::Move { source, .. }) => file.get(usize::from(source)).copied(),
            Some(quench_runtime::staged::Operation::LoadCapture { .. }) => Some(Sym::Capture(instruction.imm())),
            Some(quench_runtime::staged::Operation::GetField { .. }) => self.field(instruction),
            Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::IncDec { source, decrement, .. })) => {
                self.inc_dec(source, decrement, file)
            }
            Some(quench_runtime::staged::Operation::ElementRead { base, key, .. }) => self.get_index(base, key, file),
            Some(quench_runtime::staged::Operation::ElementWrite { value, base, key, .. }) => {
                return self.set_index(value, base, key, file).map(|()| None).ok_or(());
            }
            _ => {
                let op = instruction.op();
                if quench_runtime::staged::loads_constant(op) {
                    self.constants.get(instruction.constant_index()).and_then(|v| v.as_number()).map(Sym::Const)
                } else if quench_runtime::staged::computes_arithmetic(op) {
                    self.binary(instruction, file)
                } else if quench_runtime::staged::does_nothing(op) {
                    return Ok(None);
                } else {
                    None
                }
            }
        };
        value.map(Some).ok_or(())
    }

    /// One pass over `body`, whose first instruction is at `start`, from the register file
    /// `file`. Forward branches inside the body are if-converted: every instruction runs under
    /// its path's predicate (a 1-or-0 node; paths into a join are exclusive, so the join's
    /// predicate is their sum). A write under a predicate is `r = r ± k` as a select
    /// (`r - (0 - p * k)`, which keeps `-0` where `p` is 0), or a write to a register only
    /// that path reads (`Sym::Poison` past it).
    fn pass(&mut self, body: &[WideInstruction], start: usize, file: &mut [Sym]) -> Option<()> {
        const TRUE: Sym = Sym::Const(1.0);
        let trace = |instruction: WideInstruction| {
            if quench_runtime::trace::Channel::LoopKernel.on() {
                eprintln!("loop-kernel:   no model for {:?} {:?}", instruction.op(), instruction.binary_operator());
            }
        };
        // Each path also carries its share of the iterations' paths: a branch halves its path's
        // share, a join adds the shares, and a join of every path (share 1) runs under `TRUE`.
        let mut paths: Vec<Option<(Sym, f64)>> = vec![None; body.len() + 1];
        paths[0] = Some((TRUE, 1.0));
        let mut guards: Vec<(u16, Sym)> = Vec::new();
        for (k, &instruction) in body.iter().enumerate() {
            let Some((path, share)) = paths[k] else { continue };
            Self::poison(&mut guards, file, path)?;
            if instruction.op().control_flow_layout().jumps() {
                let target = (instruction.jump_target() as usize).checked_sub(start)?;
                if target <= k || target > body.len() {
                    trace(instruction);
                    return None;
                }
                if matches!(staged_operation(instruction), Some(quench_runtime::staged::Operation::Jump)) {
                    self.join(&mut paths[target], (path, share))?;
                } else if instruction.op() == quench_runtime::bytecode::Op::JumpBinaryFalse {
                    let relation = NumberRelation::from_operator(instruction.binary_operator_field())?;
                    let a = self.operand(file, instruction.operand_b())?;
                    let b = self.operand(file, instruction.operand_c())?;
                    let (a, b) = (self.arith_operand(a)?, self.arith_operand(b)?);
                    let holds = self.node(SymOp::Cmp(relation), a, b)?;
                    let fails = self.node(SymOp::Arith(NumberArithmetic::Subtract), Sym::Const(1.0), holds)?;
                    let (through, jumped) = (self.and(path, holds)?, self.and(path, fails)?);
                    self.join(&mut paths[k + 1], (through, share / 2.0))?;
                    self.join(&mut paths[target], (jumped, share / 2.0))?;
                } else {
                    trace(instruction);
                    return None;
                }
                continue;
            }
            if path != TRUE {
                if let Some((register, delta)) = Self::stepped(instruction, file) {
                    let old = self.arith_operand(*file.get(usize::from(register))?)?;
                    let product = if delta == 1.0 { path } else { self.node(SymOp::Arith(NumberArithmetic::Multiply), path, Sym::Const(delta))? };
                    let negated = self.node(SymOp::Arith(NumberArithmetic::Subtract), Sym::Const(0.0), product)?;
                    *file.get_mut(usize::from(register))? = self.node(SymOp::Arith(NumberArithmetic::Subtract), old, negated)?;
                    self.join(&mut paths[k + 1], (path, share))?;
                    continue;
                }
                if matches!(staged_operation(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. })) {
                    trace(instruction);
                    return None;
                }
            }
            let Ok(value) = self.step(instruction, file) else {
                trace(instruction);
                return None;
            };
            if let Some(value) = value {
                let register = instruction.result_register();
                *file.get_mut(usize::from(register))? = value;
                if path != TRUE {
                    guards.retain(|&(r, _)| r != register);
                    guards.push((register, path));
                }
            }
            self.join(&mut paths[k + 1], (path, share))?;
        }
        let (end, _) = paths[body.len()]?;
        Self::poison(&mut guards, file, end)
    }

    /// Poisons the registers written under a predicate other than `path`.
    fn poison(guards: &mut Vec<(u16, Sym)>, file: &mut [Sym], path: Sym) -> Option<()> {
        let mut result = Some(());
        guards.retain(|&(register, written)| {
            if written == path {
                return true;
            }
            match file.get_mut(usize::from(register)) {
                Some(slot) => *slot = Sym::Poison,
                None => result = None,
            }
            false
        });
        result
    }

    /// `path && condition` of two 1-or-0 values.
    fn and(&mut self, path: Sym, condition: Sym) -> Option<Sym> {
        if path == Sym::Const(1.0) { Some(condition) } else { self.node(SymOp::Arith(NumberArithmetic::Multiply), path, condition) }
    }

    /// Adds the exclusive path `path`, with its share, into a join.
    fn join(&mut self, slot: &mut Option<(Sym, f64)>, (path, share): (Sym, f64)) -> Option<()> {
        *slot = Some(match *slot {
            None => (path, share),
            Some((_, other)) if other + share == 1.0 => (Sym::Const(1.0), 1.0),
            Some((other, other_share)) => (self.node(SymOp::Arith(NumberArithmetic::Add), other, path)?, other_share + share),
        });
        Some(())
    }

    /// `(register, k)` for an instruction that is `register = register ± k`, `k` a finite
    /// constant: an increment, or an add or subtract of a constant into its own operand.
    fn stepped(instruction: WideInstruction, file: &[Sym]) -> Option<(u16, f64)> {
        let _ = file;
        match staged_operation(instruction) {
            Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::IncDec { source, decrement, .. })) => {
                (source == instruction.result_register()).then_some((source, if decrement { -1.0 } else { 1.0 }))
            }
            _ => None,
        }
    }
}

/// An affine value plus or minus an integer constant, as the staged int32 row computes it: the
/// row's checked result stays affine, and an overflowing one leaves the form.
fn shifted(kind: SymOp, a: Sym, b: Sym) -> Option<Sym> {
    let integer = |sym: Sym| match sym {
        Sym::Const(value) if value.fract() == 0.0 && value.abs() <= 65536.0 => Some(value as i32),
        _ => None,
    };
    let (register, offset, other, operation) = match (kind, a, b) {
        (SymOp::Arith(operation @ NumberArithmetic::Add), Sym::Affine(register, offset), other)
        | (SymOp::Arith(operation @ NumberArithmetic::Add), other, Sym::Affine(register, offset)) => (register, offset, other, operation),
        (SymOp::Arith(operation @ NumberArithmetic::Subtract), Sym::Affine(register, offset), other) => (register, offset, other, operation),
        _ => return None,
    };
    let mut stage = quench_runtime::staged::Concrete::new(std::ptr::null());
    let (word, overflowed) = quench_runtime::staged::NumericStage::int_arithmetic(
        &mut stage,
        operation,
        u64::from(offset as u32),
        u64::from(integer(other)? as u32),
    );
    (!overflowed).then(|| Sym::Affine(register, word as u32 as i32))
}

/// The staged operation `instruction` denotes, if its decoder has one (`Operation::decode`).
fn staged_operation(instruction: WideInstruction) -> Option<quench_runtime::staged::Operation<(), ()>> {
    quench_runtime::staged::Operation::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY)
}

/// Whether `instruction` writes a numeric local besides its result, which no kernel models.
fn writes_local(instruction: WideInstruction) -> bool {
    let stores = matches!(staged_operation(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. }));
    !stores && !quench_runtime::staged::does_nothing(instruction.op()) && instruction.numeric_local_target().is_some()
}

/// The registers `body` writes.
fn written(body: &[WideInstruction], registers: usize) -> Option<Vec<bool>> {
    let mut written = vec![false; registers];
    for instruction in body {
        for register in instruction.written_registers() {
            *written.get_mut(usize::from(register))? = true;
        }
    }
    Some(written)
}

/// The relation with its operands swapped.
fn flipped(relation: NumberRelation) -> Option<NumberRelation> {
    Some(match relation {
        NumberRelation::LessThan => NumberRelation::GreaterThan,
        NumberRelation::LessEqual => NumberRelation::GreaterEqual,
        NumberRelation::GreaterThan => NumberRelation::LessThan,
        NumberRelation::GreaterEqual => NumberRelation::LessEqual,
        _ => return None,
    })
}

impl Shape {
    /// The shape of the loop whose backward jump at `backedge` goes to `header`, if it has a
    /// kernel: `header..test` recomputes values before the test, `test` leaves the loop to
    /// just past the jump, and `test + 1..backedge` is the body.
    pub(crate) fn of(decoded: &[WideInstruction], constants: &[Value], registers: usize, header: usize, backedge: usize) -> Option<Self> {
        let leaves = |instruction: &WideInstruction| instruction.op().control_flow_layout().jumps();
        let test = header + decoded[header..backedge].iter().position(leaves)?;
        let (pre, body) = (&decoded[header..test], &decoded[test + 1..backedge]);
        let jump = decoded[test];
        if !quench_runtime::staged::compare_jump(jump.op()) || jump.jump_target() as usize != backedge + 1 {
            return None;
        }
        let relation = NumberRelation::from_operator(jump.binary_operator_field())?;
        let written = written(&decoded[header..backedge], registers)?;
        // Pass 1: every register as its own start value finds the inductions.
        let identity: Vec<Sym> = (0..registers as u16).map(|register| Sym::Affine(register, 0)).collect();
        let all = vec![true; registers];
        let mut first = Graph::new(constants, &all, &written);
        let mut file = identity.clone();
        first.pass(pre, header, &mut file)?;
        first.pass(body, test + 1, &mut file)?;
        let induction: Vec<bool> = (0..registers)
            .map(|register| {
                written[register]
                    && match file[register] {
                        Sym::Affine(r, step) => usize::from(r) == register && step != 0,
                        Sym::Strided(r, _, _) => usize::from(r) == register,
                        _ => false,
                    }
            })
            .collect();
        // Pass 2: recurrences start as `Phi`.
        let mut file: Vec<Sym> = identity
            .iter()
            .enumerate()
            .map(|(register, &sym)| if written[register] && !induction[register] { Sym::Phi(register as u16) } else { sym })
            .collect();
        let mut graph = Graph::new(constants, &induction, &written);
        graph.pass(pre, header, &mut file)?;
        let left = graph.operand(&file, jump.operand_b())?;
        let right = graph.operand(&file, jump.operand_c())?;
        graph.pass(body, test + 1, &mut file)?;
        Lowering::new(&induction, &written).lower(graph, &file, (left, relation, right))
    }
}

/// Lowers a pass's graph to a `Shape`: symbolic values to slots.
struct Lowering<'a> {
    induction: &'a [bool],
    written: &'a [bool],
    scalars: Vec<Scalar>,
    affines: Vec<Affine>,
    bases: Vec<u16>,
    phis: Vec<u16>,
    /// The scalar slot of each node hoisted so far.
    hoisted: Vec<Option<u8>>,
}

impl<'a> Lowering<'a> {
    fn new(induction: &'a [bool], written: &'a [bool]) -> Self {
        Self { induction, written, scalars: Vec::new(), affines: Vec::new(), bases: Vec::new(), phis: Vec::new(), hoisted: Vec::new() }
    }

    fn slot<T: PartialEq>(list: &mut Vec<T>, item: T) -> Option<u8> {
        let index = list.iter().position(|known| *known == item).unwrap_or_else(|| {
            list.push(item);
            list.len() - 1
        });
        u8::try_from(index).ok().filter(|&index| usize::from(index) < MAX_SLOTS)
    }

    fn affine(&mut self, (register, offset): (u16, i32)) -> Option<u8> {
        let step = if self.induction[usize::from(register)] {
            Stride::Const(0) // filled from the inductions in `lower`
        } else if self.written[usize::from(register)] {
            return None;
        } else {
            Stride::Const(0)
        };
        Self::slot(&mut self.affines, Affine { register, offset, step })
    }

    fn base(&mut self, register: u16) -> Option<u8> {
        Self::slot(&mut self.bases, register)
    }

    fn arg(&mut self, sym: Sym) -> Option<Arg> {
        Some(match sym {
            Sym::Node(id) => match self.hoisted.get(usize::from(id)).copied().flatten() {
                Some(slot) => Arg::Scalar(slot),
                None => Arg::Col(id),
            },
            Sym::Phi(register) => Arg::Phi(Self::slot(&mut self.phis, register)?),
            Sym::Const(value) => {
                let index = self.scalars.len();
                self.scalars.push(Scalar::Const(value));
                Arg::Scalar(u8::try_from(index).ok().filter(|&i| usize::from(i) < MAX_SLOTS)?)
            }
            Sym::Affine(register, 0) if !self.written[usize::from(register)] => {
                let index = self.scalars.iter().position(|s| matches!(s, Scalar::Register(r) if *r == register));
                let index = index.unwrap_or_else(|| {
                    self.scalars.push(Scalar::Register(register));
                    self.scalars.len() - 1
                });
                Arg::Scalar(u8::try_from(index).ok().filter(|&i| usize::from(i) < MAX_SLOTS)?)
            }
            Sym::Affine(register, offset) => Arg::Affine(self.affine((register, offset))?),
            Sym::Capture(imm) => self.scalar(Scalar::Capture(imm))?,
            Sym::Field(register, atom) => self.scalar(Scalar::Length { register, atom })?,
            Sym::Strided(..) | Sym::Poison => return None,
        })
    }

    /// The slot of an entry-read scalar, shared by its reads.
    fn scalar(&mut self, scalar: Scalar) -> Option<Arg> {
        let index = self.scalars.iter().position(|known| known.same_read(scalar)).unwrap_or_else(|| {
            self.scalars.push(scalar);
            self.scalars.len() - 1
        });
        Some(Arg::Scalar(u8::try_from(index).ok().filter(|&i| usize::from(i) < MAX_SLOTS)?))
    }

    fn node(&mut self, (op, a, b): (SymOp, Sym, Sym)) -> Option<Node> {
        let op = match op {
            SymOp::Arith(arith) => NodeOp::Arith(arith),
            SymOp::Bits(bits) => NodeOp::Bits(bits),
            SymOp::Load { base, index } => NodeOp::Load { base: self.base(base)?, index: self.affine(index)? },
            SymOp::Iota { index } => NodeOp::Iota { index: self.affine(index)? },
            SymOp::Cmp(relation) => NodeOp::Cmp(relation),
        };
        let (a, b) = match op {
            NodeOp::Arith(_) | NodeOp::Bits(_) | NodeOp::Cmp(_) => (self.arg(a)?, self.arg(b)?),
            _ => (Arg::Scalar(0), Arg::Scalar(0)),
        };
        // Arithmetic over scalars alone is the same every iteration: one scalar at entry.
        let hoisted = match (op, a, b) {
            (NodeOp::Arith(_) | NodeOp::Bits(_) | NodeOp::Cmp(_), Arg::Scalar(a), Arg::Scalar(b)) => {
                self.scalars.push(Scalar::Computed { op, a, b });
                Some(u8::try_from(self.scalars.len() - 1).ok().filter(|&slot| usize::from(slot) < MAX_SLOTS)?)
            }
            _ => None,
        };
        self.hoisted.push(hoisted);
        Some(Node { op, a, b, seq: false, hoisted: hoisted.is_some(), dead: false })
    }

    /// The test as `counter REL bound`, the counter an induction at offset 0.
    fn test(&mut self, (left, relation, right): (Sym, NumberRelation, Sym)) -> Option<(u8, NumberRelation, u8)> {
        let is_counter = |sym: Sym| matches!(sym, Sym::Affine(r, _) if self.induction[usize::from(r)]);
        let (counter, relation, bound) = if is_counter(left) {
            (left, relation, right)
        } else if is_counter(right) {
            (right, flipped(relation)?, left)
        } else {
            return None;
        };
        let Sym::Affine(counter, offset) = counter else { return None };
        let Arg::Scalar(bound) = self.arg(bound)? else { return None };
        Some((self.affine((counter, offset))?, relation, bound))
    }
}

mod lower;
