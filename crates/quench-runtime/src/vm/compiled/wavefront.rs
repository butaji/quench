//! The wavefront lowering of a row-recurrence stencil nest: an outer loop over rows whose
//! body runs one inner loop over the row's elements, each element a Number computed from
//! elements of record arrays and values carried from the row's previous element, stored at
//! the row's current element, as a Gauss-Seidel sweep compiles:
//!
//! ```js
//! for (var j = 1; j <= height; j++) {
//!     var lastRow = (j - 1) * rowSize, currentRow = j * rowSize, nextRow = (j + 1) * rowSize;
//!     var lastX = x[currentRow]; ++currentRow;
//!     for (var i = 1; i <= width; i++)
//!         lastX = x[currentRow] = (x0[currentRow] + a * (lastX + x[++currentRow] + x[++lastRow] + x[++nextRow])) * invC;
//! }
//! ```
//!
//! Each element depends on the one before it in its row, so a row is one chain of dependent
//! floating-point operations, whose latency bounds it. Two rows are independent chains once
//! the second trails the first by one element: native code computes rows `j` and `j + 1` as
//! a pair, element `t` of row `j` then element `t - 1` of row `j + 1`, which interleaves the
//! two chains. Every element still reads exactly the values it reads in program order, so
//! the results are those of the loop, bit for bit, when every read of a written array keeps
//! its order relative to the writes (`Nest::legal`): a read of the previous row trails its
//! write by at most one column (`di <= 1`), a read of the next row leads by at least one
//! (`di >= -1`), and rows are a stride apart that no row's accesses span (checked at entry).
//!
//! Native code leaves only where a pair starts, which is a state the loop reaches: the
//! outer header with the pair's row, or, past the last row, where the loop's test leaves. A
//! pair whose rows the loop's test or the arrays' lengths do not admit leaves before it
//! runs. Each store saves the element it replaces; an element that is not a Number (which
//! the general path converts with observable effects) restores the ones the pair stored and
//! leaves at the pair's start, where the general path runs the rows in program order.
//! Everything a pair does before leaving is reads of record arrays and arithmetic, which
//! nothing observes.

use super::osr_loop::LoopRegion;
use crate::bytecode::{Op, Operand, OperandKind, Register, WideInstruction};
use crate::value::ValueLayout;
use super::super::operations::{NumberArithmetic, NumberRelation, NumberUnary};
use cranelift_codegen::ir::{
    self, InstBuilder, UserFuncName,
    condcodes::{FloatCC, IntCC},
    types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use std::collections::{BTreeMap, BTreeSet};

/// The largest magnitude an integer input of a nest's index arithmetic may hold at entry:
/// products of two such values and their sums stay exact in a double, so integer arithmetic
/// on them is the loop's Number arithmetic.
const MAX_INDEX_INPUT: i64 = 1 << 20;
/// The most elements a row may have: a pair saves its rows' written elements on the stack.
const MAX_ROW_ELEMENTS: i64 = 512;
/// The most arrays a nest may write, each with one store an element.
const MAX_WRITTEN_ARRAYS: usize = 2;
/// The rows a pair computes.
const PAIR_ROWS: i64 = 2;
const WORD_BYTES: i64 = 8;
/// The Cranelift user-function namespace of wavefront nests ("QWAV").
const WAVEFRONT_NAMESPACE: u32 = 0x5157_4156;

/// An integer input of index arithmetic: the row, the element's offset in its row, or an
/// invariant integer the nest reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Atom {
    /// The multiplicative identity, which pads a monomial of lower degree.
    Unit,
    /// The outer loop's register at the row.
    Row,
    /// The element's offset from the row's first element.
    Column,
    Register(Register),
    Capture(u16),
}

/// A product of at most two atoms, `Unit` padding a lower degree, in order.
type Monomial = [Atom; 2];

/// An integer polynomial of degree at most two over the atoms: index arithmetic of the nest.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct Poly(BTreeMap<Monomial, i64>);

impl Poly {
    fn constant(value: i64) -> Self {
        Self::default().plus_term([Atom::Unit, Atom::Unit], value)
    }

    fn atom(atom: Atom) -> Self {
        Self::default().plus_term([Atom::Unit, atom], 1)
    }

    fn plus_term(mut self, monomial: Monomial, coefficient: i64) -> Self {
        let entry = self.0.entry(monomial).or_insert(0);
        *entry += coefficient;
        if *entry == 0 {
            self.0.remove(&monomial);
        }
        self
    }

    fn plus(&self, other: &Self) -> Self {
        other.0.iter().fold(self.clone(), |sum, (monomial, coefficient)| sum.plus_term(*monomial, *coefficient))
    }

    fn scaled(&self, factor: i64) -> Self {
        Self(self.0.iter().map(|(monomial, coefficient)| (*monomial, coefficient * factor)).filter(|(_, c)| *c != 0).collect())
    }

    fn minus(&self, other: &Self) -> Self {
        self.plus(&other.scaled(-1))
    }

    /// The product, when its degree stays at most two.
    fn times(&self, other: &Self) -> Option<Self> {
        let mut product = Self::default();
        for (left, a) in &self.0 {
            for (right, b) in &other.0 {
                let mut atoms: Vec<Atom> = left.iter().chain(right).copied().filter(|atom| *atom != Atom::Unit).collect();
                (atoms.len() <= 2).then_some(())?;
                atoms.sort();
                while atoms.len() < 2 {
                    atoms.insert(0, Atom::Unit);
                }
                product = product.plus_term([atoms[0], atoms[1]], a.checked_mul(*b)?);
            }
        }
        Some(product)
    }

    fn mentions(&self, atom: Atom) -> bool {
        self.0.keys().any(|monomial| monomial.contains(&atom))
    }

    /// The constant, when the polynomial is one.
    fn as_constant(&self) -> Option<i64> {
        match self.0.len() {
            0 => Some(0),
            1 => self.0.get(&[Atom::Unit, Atom::Unit]).copied(),
            _ => None,
        }
    }

    /// The coefficient of `Column`, when the polynomial is linear in it with no product of it
    /// and another atom: 0 or 1 for an access (`Access`).
    fn column_step(&self) -> Option<i64> {
        let mut step = 0;
        for (monomial, coefficient) in &self.0 {
            if monomial.contains(&Atom::Column) {
                (*monomial == [Atom::Unit, Atom::Column]).then_some(())?;
                step = *coefficient;
            }
        }
        Some(step)
    }

    /// The part that multiplies `Row`, as a polynomial of the other atoms, and the rest.
    fn split_row(&self) -> (Self, Self) {
        let mut row = Self::default();
        let mut rest = Self::default();
        for (monomial, coefficient) in &self.0 {
            match monomial {
                [Atom::Unit, Atom::Row] => row = row.plus_term([Atom::Unit, Atom::Unit], *coefficient),
                [other, Atom::Row] | [Atom::Row, other] => row = row.plus_term([Atom::Unit, *other], *coefficient),
                _ => rest = rest.plus_term(*monomial, *coefficient),
            }
        }
        (row, rest)
    }

    fn atoms(&self) -> impl Iterator<Item = Atom> + '_ {
        self.0.keys().flatten().copied()
    }
}

/// A value the nest reads that no instruction of it writes: a register or a capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Invariant {
    Register(Register),
    Capture(u16),
}

impl Invariant {
    fn atom(self) -> Atom {
        match self {
            Self::Register(register) => Atom::Register(register),
            Self::Capture(slot) => Atom::Capture(slot),
        }
    }
}

/// A Number of the nest, as native code computes it.
#[derive(Clone, Debug)]
enum Node {
    /// The element an access reads (`Access`).
    Load(usize),
    /// A row value: one the prologue computes, which the body reads or carries from element
    /// to element (`RowValue`).
    Row(usize),
    /// An invariant read as a Number.
    Invariant(Invariant),
    Constant(f64),
    /// Index arithmetic read as a Number.
    Integer(Poly),
    Arithmetic(NumberArithmetic, usize, usize),
    Unary(NumberUnary, usize),
}

/// An element access: the array register and the element's index.
#[derive(Clone, Debug)]
struct Access {
    array: Register,
    index: Poly,
}

/// What one phase of a row runs, in program order.
#[derive(Clone, Debug)]
enum Step {
    /// Compute a node (`Program::nodes`).
    Node(usize),
    /// Store a node's Number at an access's element.
    Store(usize, usize),
}

#[derive(Clone, Debug, Default)]
struct Program {
    nodes: Vec<Node>,
    steps: Vec<Step>,
}

impl Program {
    fn node(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        let index = self.nodes.len() - 1;
        self.steps.push(Step::Node(index));
        index
    }
}

/// A Number the prologue computes for its row, and, when the body redefines its register, the
/// body node of the next element's value.
#[derive(Clone, Debug)]
struct RowValue {
    init: usize,
    next: Option<usize>,
}

/// A register's symbolic value during recognition.
#[derive(Clone, Debug)]
enum Symbol {
    Integer(Poly),
    Number(usize),
    Invariant(Invariant),
}

/// A recognized nest: its loops and the programs of a row and of an element.
pub(super) struct Nest {
    header: u32,
    /// Where the outer loop's test leaves when it fails.
    exit: u32,
    /// The outer loop's register.
    row: Register,
    /// The outer loop runs while the row is at most (`inclusive`) or below this.
    row_bound: Poly,
    row_inclusive: bool,
    /// Elements a row computes.
    count: Poly,
    accesses: Vec<Access>,
    prologue: Program,
    body: Program,
    rows: Vec<RowValue>,
    /// Each written array with its store's access.
    stores: Vec<(Register, usize)>,
    /// The row stride: the coefficient of `Row` in every store's index.
    stride: Poly,
    /// The column offsets of written arrays' accesses relative to their store, per phase.
    body_columns: (i64, i64),
    prologue_columns: Option<(i64, i64)>,
}

/// The nest of `region`, when it is a row-recurrence stencil whose pairs keep its results
/// (module documentation).
pub(super) fn recognize(region: &LoopRegion) -> Option<Nest> {
    Recognizer::new(region)?.nest()
}

struct Recognizer<'a> {
    region: &'a LoopRegion,
    instructions: &'a [WideInstruction],
    /// Registers some instruction of the region writes.
    written: BTreeSet<Register>,
    arrays: BTreeSet<Register>,
    accesses: Vec<Access>,
}

impl<'a> Recognizer<'a> {
    fn new(region: &'a LoopRegion) -> Option<Self> {
        let instructions = region.instructions();
        let written = instructions.iter().filter_map(|instruction| written_register(*instruction)).collect();
        let arrays = region.arrays.iter().map(|array| array.register).collect();
        (region.fields.is_empty() && region.field_store_sites.is_empty() && region.inlining.is_none()).then_some(())?;
        Some(Self { region, instructions, written, arrays, accesses: Vec::new() })
    }

    fn at(&self, pc: u32) -> Option<WideInstruction> {
        self.instructions.get(pc.checked_sub(self.region.header)? as usize).copied()
    }

    fn nest(mut self) -> Option<Nest> {
        let (header, backedge) = (self.region.header, self.region.backedge);
        // The outer loop: loads, its test, the prologue, one inner loop, its increment and
        // its backward jump.
        let back = self.at(backedge)?;
        (back.op() == Op::Jump && back.jump_target() == header).then_some(())?;
        let mut state: BTreeMap<Register, Symbol> = BTreeMap::new();
        let mut scratch = Program::default();
        let mut pc = header;
        while matches!(self.at(pc)?.op(), Op::LoadCapture | Op::LoadConst) {
            self.execute(self.at(pc)?, &mut state, &mut scratch)?;
            pc += 1;
        }
        // The header's loads name invariants and integers only: its Numbers would be nodes
        // of no row program.
        (!state.values().any(|symbol| matches!(symbol, Symbol::Number(_)))).then_some(())?;
        let test = self.at(pc)?;
        (test.op() == Op::JumpBinaryFalse && test.jump_target() > backedge).then_some(())?;
        let row_inclusive = inclusive(test)?;
        let exit = test.jump_target();
        let row = test.operand_b().register_index()?;
        let row_bound = self.invariant_integer(&state, test.operand_c())?;
        let increment = self.at(backedge - 1)?;
        (increment_of(increment) == Some(row)).then_some(())?;
        let test_pc = pc;
        // The inner loop is the one backward jump between the test and the outer increment.
        let jumps: Vec<u32> = (test_pc + 1..backedge - 1)
            .filter(|pc| self.at(*pc).is_some_and(|instruction| instruction.op() == Op::Jump && instruction.jump_target() <= *pc))
            .collect();
        let &[inner_back] = jumps.as_slice() else {
            return None;
        };
        (inner_back == backedge - 2).then_some(())?;
        let inner_header = self.at(inner_back)?.jump_target();
        (inner_header > test_pc).then_some(())?;
        // The row register is written only by the outer increment.
        (test_pc + 1..backedge - 1).all(|pc| self.at(pc).and_then(written_register) != Some(row)).then_some(())?;
        // Every other register the region writes is dead at the header, where native code
        // leaves: only the row register is written back.
        self.written
            .iter()
            .all(|register| *register == row || !(self.region.is_live(header, *register) || self.region.is_live(exit, *register)))
            .then_some(())?;

        // The prologue, on a symbolic row.
        state.insert(row, Symbol::Integer(Poly::atom(Atom::Row)));
        let mut prologue = Program::default();
        for pc in test_pc + 1..inner_header {
            self.execute(self.at(pc)?, &mut state, &mut prologue)?;
        }

        // The inner loop, on a symbolic element: each register it writes starts as the
        // prologue left it, an index advancing by one an element, or a carried Number.
        let inner_written: BTreeSet<Register> =
            (inner_header..inner_back).filter_map(|pc| self.at(pc).and_then(written_register)).collect();
        // The registers the inner loop reads before writing: the ones an element continues
        // from the previous; any other it writes is a temporary of one element.
        let mut defined = BTreeSet::new();
        let mut continued = BTreeSet::new();
        for pc in inner_header..inner_back {
            let instruction = self.at(pc)?;
            for register in read_registers(instruction) {
                if !defined.contains(&register) {
                    continued.insert(register);
                }
            }
            if let Some(register) = written_register(instruction) {
                defined.insert(register);
            }
        }
        let mut body = Program::default();
        let mut rows: Vec<RowValue> = Vec::new();
        let mut row_nodes: BTreeMap<usize, usize> = BTreeMap::new();
        let mut inner_state: BTreeMap<Register, Symbol> = BTreeMap::new();
        let mut inductions: BTreeMap<Register, Poly> = BTreeMap::new();
        let mut carried: BTreeMap<Register, usize> = BTreeMap::new();
        for (register, symbol) in &state {
            if inner_written.contains(register) && !continued.contains(register) {
                continue;
            }
            let symbol = match symbol {
                Symbol::Integer(poly) if inner_written.contains(register) => {
                    inductions.insert(*register, poly.clone());
                    Symbol::Integer(poly.plus(&Poly::atom(Atom::Column)))
                }
                Symbol::Number(node) => {
                    let index = *row_nodes.entry(*node).or_insert_with(|| {
                        rows.push(RowValue { init: *node, next: None });
                        rows.len() - 1
                    });
                    if inner_written.contains(register) {
                        carried.insert(*register, index);
                    }
                    Symbol::Number(body.node(Node::Row(index)))
                }
                // A register the loop redefines before reading it.
                _ if inner_written.contains(register) => continue,
                other => other.clone(),
            };
            inner_state.insert(*register, symbol);
        }
        let mut pc = inner_header;
        while matches!(self.at(pc)?.op(), Op::LoadCapture | Op::LoadConst) {
            self.execute(self.at(pc)?, &mut inner_state, &mut body)?;
            pc += 1;
        }
        let inner_test = self.at(pc)?;
        (inner_test.op() == Op::JumpBinaryFalse && inner_test.jump_target() == backedge - 1).then_some(())?;
        let element_inclusive = inclusive(inner_test)?;
        let element = inner_test.operand_b().register_index()?;
        let element_bound = self.invariant_integer(&inner_state, inner_test.operand_c())?;
        let start = inductions.get(&element)?.clone();
        (!start.mentions(Atom::Row)).then_some(())?;
        let count = element_bound.minus(&start).plus(&Poly::constant(i64::from(element_inclusive)));
        for pc in pc + 1..inner_back {
            self.execute(self.at(pc)?, &mut inner_state, &mut body)?;
        }
        // Each index advanced by one; each carried Number's next value is the body's.
        for (register, start) in &inductions {
            let Some(Symbol::Integer(end)) = inner_state.get(register) else {
                return None;
            };
            (end.minus(start) == Poly::atom(Atom::Column).plus(&Poly::constant(1))).then_some(())?;
        }
        for (register, index) in &carried {
            let next = match inner_state.get(register)? {
                Symbol::Number(node) => *node,
                _ => return None,
            };
            rows[*index].next = Some(next);
        }
        // Only a row whose elements carry a value is a chain whose latency pairing hides;
        // a row of independent elements gains nothing from pairs.
        (rows.iter().any(|value| value.next.is_some())).then_some(())?;
        let nest = Nest {
            header,
            exit,
            row,
            row_bound,
            row_inclusive,
            count,
            accesses: std::mem::take(&mut self.accesses),
            prologue,
            body,
            rows,
            stores: Vec::new(),
            stride: Poly::default(),
            body_columns: (0, 0),
            prologue_columns: None,
        };
        nest.legal()
    }

    /// Executes `instruction` on symbolic registers, recording its Numbers and stores in
    /// `program`, in program order.
    fn execute(
        &mut self,
        instruction: WideInstruction,
        state: &mut BTreeMap<Register, Symbol>,
        program: &mut Program,
    ) -> Option<()> {
        let result = match instruction.op() {
            Op::LoadConst => {
                let number = self.region.number_constant(instruction.constant_index())?;
                constant_symbol(number, program)
            }
            Op::LoadCapture => {
                (instruction.capture_depth() == 0).then_some(())?;
                Symbol::Invariant(Invariant::Capture(instruction.capture_slot()))
            }
            Op::Move => self.read(state, instruction.register_b())?,
            Op::IncDec => {
                let step = if instruction.boolean_flag()? { -1 } else { 1 };
                match self.read(state, instruction.register_b())? {
                    Symbol::Number(node) => {
                        let one = program.node(Node::Constant(1.0));
                        let operation = if step < 0 { NumberArithmetic::Subtract } else { NumberArithmetic::Add };
                        Symbol::Number(program.node(Node::Arithmetic(operation, node, one)))
                    }
                    symbol => Symbol::Integer(integer(&symbol)?.plus(&Poly::constant(step))),
                }
            }
            Op::Binary => {
                let operation = NumberArithmetic::from_js_instruction(Op::Binary, instruction.binary_operator())?;
                let left = self.operand(state, instruction.operand_b(), program)?;
                let right = self.operand(state, instruction.operand_c(), program)?;
                let exact = match (integer(&left), integer(&right), operation) {
                    (Some(left), Some(right), NumberArithmetic::Add) => Some(left.plus(&right)),
                    (Some(left), Some(right), NumberArithmetic::Subtract) => Some(left.minus(&right)),
                    (Some(left), Some(right), NumberArithmetic::Multiply) => left.times(&right),
                    _ => None,
                };
                match exact {
                    Some(poly) => Symbol::Integer(poly),
                    None => {
                        let (left, right) = (number(&left, program), number(&right, program));
                        Symbol::Number(program.node(Node::Arithmetic(operation, left, right)))
                    }
                }
            }
            Op::Unary => {
                let operation = NumberUnary::from_js_instruction(Op::Unary, instruction.unary_operator())?;
                let value = self.read(state, instruction.register_b())?;
                let value = number(&value, program);
                Symbol::Number(program.node(Node::Unary(operation, value)))
            }
            Op::GetIndex => {
                let access = self.access(state, instruction.operand_b().register_index()?, instruction.operand_c())?;
                Symbol::Number(program.node(Node::Load(access)))
            }
            Op::SetIndex => {
                let value = self.read(state, instruction.register_a())?;
                let value = number(&value, program);
                let access = self.access(state, instruction.register_b(), Operand::register(instruction.register_c()))?;
                program.steps.push(Step::Store(access, value));
                return Some(());
            }
            _ => return None,
        };
        state.insert(instruction.result_register(), result);
        Some(())
    }

    /// An access of the array in `array` at the element `index` names.
    fn access(&mut self, state: &BTreeMap<Register, Symbol>, array: Register, index: Operand) -> Option<usize> {
        (self.arrays.contains(&array) && !self.written.contains(&array)).then_some(())?;
        let mut scratch = Program::default();
        let index = integer(&self.operand(state, index, &mut scratch)?)?;
        let step = index.column_step()?;
        (step == 0 || step == 1).then_some(())?;
        self.accesses.push(Access { array, index });
        Some(self.accesses.len() - 1)
    }

    fn read(&self, state: &BTreeMap<Register, Symbol>, register: Register) -> Option<Symbol> {
        if let Some(symbol) = state.get(&register) {
            return Some(symbol.clone());
        }
        // A register no instruction writes is the nest's invariant.
        (!self.written.contains(&register)).then_some(Symbol::Invariant(Invariant::Register(register)))
    }

    fn operand(&self, state: &BTreeMap<Register, Symbol>, operand: Operand, program: &mut Program) -> Option<Symbol> {
        match operand.kind()? {
            OperandKind::Register => self.read(state, operand.payload()),
            OperandKind::Constant => {
                let number = self.region.number_constant(usize::from(operand.payload()))?;
                Some(constant_symbol(number, program))
            }
            _ => None,
        }
    }

    /// An integer the nest does not change: an invariant or a constant, as a polynomial.
    fn invariant_integer(&self, state: &BTreeMap<Register, Symbol>, operand: Operand) -> Option<Poly> {
        let mut scratch = Program::default();
        let poly = integer(&self.operand(state, operand, &mut scratch)?)?;
        (!poly.mentions(Atom::Row) && !poly.mentions(Atom::Column)).then_some(poly)
    }
}

/// The register an instruction of a nest writes.
fn written_register(instruction: WideInstruction) -> Option<Register> {
    matches!(
        instruction.op(),
        Op::LoadConst | Op::LoadCapture | Op::Move | Op::IncDec | Op::Binary | Op::Unary | Op::GetIndex
    )
    .then(|| instruction.result_register())
}

/// The registers an instruction of a nest reads.
fn read_registers(instruction: WideInstruction) -> Vec<Register> {
    let operand = |operand: Operand| operand.register_index();
    match instruction.op() {
        Op::Move | Op::IncDec | Op::Unary => vec![instruction.register_b()],
        Op::Binary | Op::JumpBinaryFalse => {
            [operand(instruction.operand_b()), operand(instruction.operand_c())].into_iter().flatten().collect()
        }
        Op::GetIndex => [operand(instruction.operand_b()), operand(instruction.operand_c())].into_iter().flatten().collect(),
        Op::SetIndex => vec![instruction.register_a(), instruction.register_b(), instruction.register_c()],
        _ => Vec::new(),
    }
}

/// The register an `IncDec` increments in place.
fn increment_of(instruction: WideInstruction) -> Option<Register> {
    (instruction.op() == Op::IncDec
        && instruction.boolean_flag() == Some(false)
        && instruction.result_register() == instruction.register_b())
    .then(|| instruction.register_b())
}

/// Whether a loop test continues while its register is at most (`true`) or below (`false`)
/// its bound.
fn inclusive(test: WideInstruction) -> Option<bool> {
    match NumberRelation::from_operator(test.binary_operator_field())? {
        NumberRelation::LessEqual => Some(true),
        NumberRelation::LessThan => Some(false),
        _ => None,
    }
}

/// A constant: an integer index input when it is a small integer, else a Number.
fn constant_symbol(number: f64, program: &mut Program) -> Symbol {
    let integer = number as i64;
    if integer as f64 == number && number.to_bits() != (-0.0f64).to_bits() && integer.abs() <= MAX_INDEX_INPUT {
        Symbol::Integer(Poly::constant(integer))
    } else {
        Symbol::Number(program.node(Node::Constant(number)))
    }
}

fn integer(symbol: &Symbol) -> Option<Poly> {
    match symbol {
        Symbol::Integer(poly) => Some(poly.clone()),
        Symbol::Invariant(invariant) => Some(Poly::atom(invariant.atom())),
        Symbol::Number(_) => None,
    }
}

fn number(symbol: &Symbol, program: &mut Program) -> usize {
    match symbol {
        Symbol::Number(node) => *node,
        Symbol::Invariant(invariant) => program.node(Node::Invariant(*invariant)),
        Symbol::Integer(poly) => match poly.as_constant() {
            Some(constant) => program.node(Node::Constant(constant as f64)),
            None => program.node(Node::Integer(poly.clone())),
        },
    }
}

impl Nest {
    /// The nest, when pairing its rows keeps every read's order relative to the writes
    /// (module documentation); its stores, stride and column offsets filled in.
    fn legal(mut self) -> Option<Self> {
        let stores: Vec<(Register, usize)> = self
            .body
            .steps
            .iter()
            .filter_map(|step| match step {
                Step::Store(access, _) => Some((self.accesses[*access].array, *access)),
                Step::Node(_) => None,
            })
            .collect();
        (!stores.is_empty() && stores.len() <= MAX_WRITTEN_ARRAYS).then_some(())?;
        // No prologue stores; one store an element per written array.
        (!self.prologue.steps.iter().any(|step| matches!(step, Step::Store(..)))).then_some(())?;
        let written: BTreeSet<Register> = stores.iter().map(|(array, _)| *array).collect();
        (written.len() == stores.len()).then_some(())?;
        // Every store writes the row's current element: `Row * stride + Column + offset`.
        let mut stride = None;
        for (_, access) in &stores {
            let (row, rest) = self.accesses[*access].index.split_row();
            (rest.column_step() == Some(1) && !row.mentions(Atom::Column) && !row.mentions(Atom::Row)).then_some(())?;
            match &stride {
                None => stride = Some(row),
                Some(existing) => (*existing == row).then_some(())?,
            }
        }
        let stride = stride?;
        (!stride.0.is_empty()).then_some(())?;
        let mut body_columns: Option<(i64, i64)> = None;
        let mut prologue_columns: Option<(i64, i64)> = None;
        for (phase, program) in [(Phase::Prologue, &self.prologue), (Phase::Body, &self.body)] {
            for step in &program.steps {
                let access = match step {
                    Step::Node(node) => match program.nodes[*node] {
                        Node::Load(access) => access,
                        _ => continue,
                    },
                    Step::Store(access, _) => *access,
                };
                let access = &self.accesses[access];
                let Some((_, store)) = stores.iter().find(|(array, _)| *array == access.array) else {
                    continue;
                };
                let store = &self.accesses[*store].index;
                let mut offset = access.index.minus(store);
                if phase == Phase::Prologue {
                    // A prologue read is at the row's element zero: no Column of its own.
                    (access.index.column_step() == Some(0)).then_some(())?;
                    offset = offset.plus(&Poly::atom(Atom::Column));
                }
                let (rows, columns) = decompose(&offset, &stride)?;
                let legal = match phase {
                    // The prologue of the pair's second row runs before the first row's body,
                    // so it reads its own row alone.
                    Phase::Prologue => rows == 0,
                    Phase::Body => match rows {
                        -1 => columns <= 1,
                        1 => columns >= -1,
                        _ => true,
                    },
                };
                legal.then_some(())?;
                let range = if phase == Phase::Prologue { &mut prologue_columns } else { &mut body_columns };
                *range = Some(range.map_or((columns, columns), |(low, high)| (low.min(columns), high.max(columns))));
            }
        }
        self.body_columns = body_columns?;
        self.prologue_columns = prologue_columns;
        self.stores = stores;
        self.stride = stride;
        Some(self)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prologue,
    Body,
}

/// `offset` as rows of `stride` and columns: `rows * stride + columns` with `columns` a
/// constant, when it is one.
fn decompose(offset: &Poly, stride: &Poly) -> Option<(i64, i64)> {
    (!offset.mentions(Atom::Row) && !offset.mentions(Atom::Column)).then_some(())?;
    if let Some(stride) = stride.as_constant() {
        (stride > 0).then_some(())?;
        let offset = offset.as_constant()?;
        let rows = offset.div_euclid(stride);
        let columns = offset - rows * stride;
        // The nearest row: a column offset past half the stride is the next row's.
        return Some(if columns * 2 > stride { (rows + 1, columns - stride) } else { (rows, columns) });
    }
    // A stride of one monomial: its coefficient in the offset, in whole strides.
    let (&monomial, &scale) = match stride.0.iter().collect::<Vec<_>>().as_slice() {
        [(monomial, scale)] => (*monomial, *scale),
        _ => return None,
    };
    let coefficient = offset.0.get(&monomial).copied().unwrap_or(0);
    (coefficient % scale == 0).then_some(())?;
    let rows = coefficient / scale;
    let columns = offset.minus(&stride.scaled(rows)).as_constant()?;
    Some((rows, columns))
}


// ---- Lowering ----

/// Lowers a recognized nest to a shared-entry function.
pub(super) fn lower(
    region: &LoopRegion,
    nest: &Nest,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    poll: Option<i64>,
) -> Option<ir::Function> {
    let poll = poll?;
    let pointer = isa.pointer_type();
    let signature = super::native_entry_signature(pointer, isa.default_call_conv());
    let mut function = ir::Function::with_name_signature(UserFuncName::user(WAVEFRONT_NAMESPACE, nest.header), signature);
    let mut context = FunctionBuilderContext::new();
    let builder = FunctionBuilder::new(&mut function, &mut context);
    Emitter::new(builder, region, nest, poll)?.emit(isa.frontend_config())?;
    Some(function)
}

/// An array as native code indexes it: its element pointer and length, as `i64`s.
#[derive(Clone, Copy)]
struct Array {
    elements: ir::Value,
    length: ir::Value,
}

struct Emitter<'a, 'b> {
    builder: FunctionBuilder<'b>,
    region: &'a LoopRegion,
    nest: &'a Nest,
    poll: i64,
    registers: ir::Value,
    result: ir::Value,
    arrays: BTreeMap<Register, Array>,
    /// Integer atoms, as `i64`.
    integers: BTreeMap<Atom, ir::Value>,
    /// Number invariants, as `f64`.
    numbers: BTreeMap<Invariant, ir::Value>,
    not_entered: ir::Block,
}

impl<'a, 'b> Emitter<'a, 'b> {
    fn new(mut builder: FunctionBuilder<'b>, region: &'a LoopRegion, nest: &'a Nest, poll: i64) -> Option<Self> {
        let start = builder.create_block();
        builder.append_block_params_for_function_params(start);
        builder.switch_to_block(start);
        let &[_, registers, _, _, result] = builder.block_params(start) else {
            return None;
        };
        let not_entered = builder.create_block();
        builder.set_cold_block(not_entered);
        Some(Self {
            builder,
            region,
            nest,
            poll,
            registers,
            result,
            arrays: BTreeMap::new(),
            integers: BTreeMap::new(),
            numbers: BTreeMap::new(),
            not_entered,
        })
    }

    fn emit(mut self, frontend: cranelift_codegen::isa::TargetFrontendConfig) -> Option<()> {
        let start = self.builder.current_block()?;
        let entry = self.builder.block_params(start)[3];
        // Arrays from the entry record, after the captures and fields.
        let arrays_start = super::osr_loop::ENTRY_CAPTURES + self.region.captures.len() + self.region.fields.len();
        for (index, array) in self.region.arrays.iter().enumerate() {
            let base = (arrays_start + super::osr_loop::ARRAY_WORDS * index) as i32 * WORD_BYTES as i32;
            let elements = self.builder.ins().load(types::I64, flags(), entry, base);
            let length = self.builder.ins().load(types::I64, flags(), entry, base + WORD_BYTES as i32);
            self.arrays.insert(array.register, Array { elements, length });
        }
        // Integer inputs: the row register and every invariant atom.
        let row_bits = self.builder.ins().load(types::I64, flags(), self.registers, i32::from(self.nest.row) * WORD_BYTES as i32);
        let row = self.unbox_integer(row_bits)?;
        let atoms: BTreeSet<Atom> = self
            .nest
            .accesses
            .iter()
            .flat_map(|access| access.index.atoms())
            .chain(self.nest.row_bound.atoms())
            .chain(self.nest.count.atoms())
            .chain(self.nest.stride.atoms())
            .chain(self.integer_node_atoms())
            .filter(|atom| matches!(atom, Atom::Register(_) | Atom::Capture(_)))
            .collect();
        for atom in atoms {
            let bits = self.invariant_bits(atom, entry)?;
            let value = self.unbox_integer(bits)?;
            self.integers.insert(atom, value);
        }
        let numbers: BTreeSet<Invariant> = [&self.nest.prologue, &self.nest.body]
            .iter()
            .flat_map(|program| program.nodes.iter())
            .filter_map(|node| match node {
                Node::Invariant(invariant) => Some(*invariant),
                _ => None,
            })
            .collect();
        for invariant in numbers {
            let bits = self.invariant_bits(invariant.atom(), entry)?;
            let not_entered = self.not_entered;
            let value = unbox_number(&mut self.builder, bits, not_entered);
            self.numbers.insert(invariant, value);
        }
        // Written arrays are distinct from every other array.
        for (written, _) in &self.nest.stores {
            for (other, array) in &self.arrays.clone() {
                if other == written {
                    continue;
                }
                let same = self.builder.ins().icmp(IntCC::Equal, self.arrays[written].elements, array.elements);
                self.fail_if(same, self.not_entered);
            }
        }
        // Elements a row computes, and the stride no row's accesses span.
        let count = self.evaluate(&self.nest.count, None, None);
        let empty = self.builder.ins().icmp_imm_s(IntCC::SignedLessThan, count, 1);
        self.fail_if(empty, self.not_entered);
        let long = self.builder.ins().icmp_imm_s(IntCC::SignedGreaterThan, count, MAX_ROW_ELEMENTS);
        self.fail_if(long, self.not_entered);
        let stride = self.evaluate(&self.nest.stride, None, None);
        let (low, high) = self.nest.body_columns;
        let (low, high) = match self.nest.prologue_columns {
            Some((pro_low, pro_high)) => (low.min(pro_low), high.max(pro_high)),
            None => (low, high),
        };
        // Columns a row touches span `count - 1 + (high - low)`; the stride must exceed it.
        let span = self.builder.ins().iadd_imm_s(count, high - low - 1);
        let overlapping = self.builder.ins().icmp(IntCC::SignedLessThanOrEqual, stride, span);
        self.fail_if(overlapping, self.not_entered);
        let save_bytes = (MAX_ROW_ELEMENTS * PAIR_ROWS * MAX_WRITTEN_ARRAYS as i64 * WORD_BYTES) as u32;
        let save = self.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            save_bytes,
            WORD_BYTES.trailing_zeros() as u8,
        ));
        let save = self.builder.ins().stack_addr(types::I64, save, 0);
        let row_bound = self.evaluate(&self.nest.row_bound, None, None);

        // Pairs.
        let pair = self.builder.create_block();
        self.builder.append_block_param(pair, types::I64);
        self.builder.ins().jump(pair, &[ir::BlockArg::Value(row)]);
        self.builder.switch_to_block(pair);
        let row = self.builder.block_params(pair)[0];
        let leave = self.builder.create_block();
        self.builder.append_block_param(leave, types::I64);
        self.builder.set_cold_block(leave);
        let finished = self.builder.create_block();
        self.builder.append_block_param(finished, types::I64);
        // A row past the loop's test ends the loop, as its test would; a pair whose second
        // row is past leaves its one row to the general path.
        let condition = if self.nest.row_inclusive { IntCC::SignedGreaterThan } else { IntCC::SignedGreaterThanOrEqual };
        let done = self.builder.ins().icmp(condition, row, row_bound);
        self.leave_if(done, finished, row);
        let second = self.builder.ins().iadd_imm_s(row, 1);
        let past = self.builder.ins().icmp(condition, second, row_bound);
        self.leave_if(past, leave, row);
        // Every access of both rows lies within its array.
        let last = self.builder.ins().iadd_imm_s(count, -1);
        for access in self.nest.accesses.clone() {
            for offset in 0..PAIR_ROWS {
                let at_row = self.builder.ins().iadd_imm_s(row, offset);
                let first = self.evaluate(&access.index, Some(at_row), Some(None));
                let final_column = if access.index.column_step() == Some(1) { Some(last) } else { None };
                let final_index = self.evaluate(&access.index, Some(at_row), Some(final_column));
                let length = self.arrays[&access.array].length;
                let below = self.builder.ins().icmp_imm_s(IntCC::SignedLessThan, first, 0);
                self.leave_if(below, leave, row);
                let beyond = self.builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, final_index, length);
                self.leave_if(beyond, leave, row);
            }
        }
        // A termination request faults here, between pairs.
        let page = self.builder.ins().iconst(types::I64, self.poll);
        self.builder.ins().atomic_load(types::I8, ir::MemFlagsData::trusted(), page);
        // Each store saves the element it replaces (`Pair::save`); a non-Number element
        // restores the ones stored so far, by row (`restore`'s parameters).
        let restore = self.builder.create_block();
        self.builder.set_cold_block(restore);
        for _ in 0..self.nest.stores.len() as i64 * PAIR_ROWS {
            self.builder.append_block_param(restore, types::I64);
        }
        let zero = self.builder.ins().iconst(types::I64, 0);
        let pair_state = Pair { save, restore };
        // The rows' prologues, then the pair's elements, the second row one behind.
        let mut values: [Vec<ir::Value>; PAIR_ROWS as usize] = [Vec::new(), Vec::new()];
        for offset in 0..PAIR_ROWS {
            let at_row = self.builder.ins().iadd_imm_s(row, offset);
            values[offset as usize] = self.run_prologue(at_row, Place { pair: pair_state, row: offset, other_stored: zero })?;
        }
        let first_row = row;
        let second_row = self.builder.ins().iadd_imm_s(row, 1);
        let carried = self.run_element(first_row, zero, &values[0], Place { pair: pair_state, row: 0, other_stored: zero })?;
        let carried_first = carried;
        let carried_second = self.carried(&values[1]);
        let element = self.builder.create_block();
        let count_carried = carried_first.len();
        self.builder.append_block_param(element, types::I64);
        for _ in 0..count_carried + carried_second.len() {
            self.builder.append_block_param(element, types::F64);
        }
        let tail = self.builder.create_block();
        for _ in 0..count_carried + carried_second.len() {
            self.builder.append_block_param(tail, types::F64);
        }
        let one = self.builder.ins().iconst(types::I64, 1);
        let mut arguments: Vec<ir::BlockArg> = vec![ir::BlockArg::Value(one)];
        arguments.extend(carried_first.iter().chain(&carried_second).map(|value| ir::BlockArg::Value(*value)));
        let done = self.builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, one, count);
        let tail_arguments: Vec<ir::BlockArg> = arguments[1..].to_vec();
        self.builder.ins().brif(done, tail, &tail_arguments, element, &arguments);
        self.builder.switch_to_block(element);
        let params = self.builder.block_params(element).to_vec();
        let column = params[0];
        let first_carried: Vec<ir::Value> = params[1..1 + count_carried].to_vec();
        let second_carried: Vec<ir::Value> = params[1 + count_carried..].to_vec();
        let first_values = self.with_carried(&values[0], &first_carried);
        // The second row has stored the elements before its previous one; the first, all up
        // to its current one.
        let previous = self.builder.ins().iadd_imm_s(column, -1);
        let first_next = self.run_element(first_row, column, &first_values, Place { pair: pair_state, row: 0, other_stored: previous })?;
        let second_values = self.with_carried(&values[1], &second_carried);
        let stored_first = self.builder.ins().iadd_imm_s(column, 1);
        let second_next =
            self.run_element(second_row, previous, &second_values, Place { pair: pair_state, row: 1, other_stored: stored_first })?;
        let next_column = self.builder.ins().iadd_imm_s(column, 1);
        let more = self.builder.ins().icmp(IntCC::SignedLessThan, next_column, count);
        let mut arguments: Vec<ir::BlockArg> = vec![ir::BlockArg::Value(next_column)];
        arguments.extend(first_next.iter().chain(&second_next).map(|value| ir::BlockArg::Value(*value)));
        let tail_arguments: Vec<ir::BlockArg> = arguments[1..].to_vec();
        self.builder.ins().brif(more, element, &arguments, tail, &tail_arguments);
        self.builder.switch_to_block(tail);
        let params = self.builder.block_params(tail).to_vec();
        let second_carried: Vec<ir::Value> = params[count_carried..].to_vec();
        let second_values = self.with_carried(&values[1], &second_carried);
        self.run_element(second_row, last, &second_values, Place { pair: pair_state, row: 1, other_stored: count })?;
        let next_row = self.builder.ins().iadd_imm_s(row, PAIR_ROWS);
        self.builder.ins().jump(pair, &[ir::BlockArg::Value(next_row)]);

        // A non-Number element: restore the pair's stored elements and leave at its start.
        self.builder.switch_to_block(restore);
        let stored = self.builder.block_params(restore).to_vec();
        self.restore_rows(row, &stored, save);
        self.builder.ins().jump(leave, &[ir::BlockArg::Value(row)]);

        // Leaving: the row register holds the pair's row, and the general path runs the
        // header; or the loop is done, and the lane resumes where its test leaves.
        for (block, outcome) in [
            (leave, super::RegionExit::Fallback(self.nest.header)),
            (finished, super::RegionExit::Resume(self.nest.exit)),
        ] {
            self.builder.switch_to_block(block);
            let row = self.builder.block_params(block)[0];
            let payload = self.builder.ins().ireduce(types::I32, row);
            let payload = self.builder.ins().uextend(types::I64, payload);
            let bits = self.builder.ins().bor_imm_u(payload, ValueLayout::INT_TAG as i64);
            self.builder.ins().store(flags(), bits, self.registers, i32::from(self.nest.row) * WORD_BYTES as i32);
            let word = self.builder.ins().iconst(types::I64, outcome.encode() as i64);
            self.finish(word);
        }

        self.builder.switch_to_block(self.not_entered);
        let word = self.builder.ins().iconst(types::I64, super::RegionExit::EntryMiss(0).encode() as i64);
        self.finish(word);
        self.builder.seal_all_blocks();
        self.builder.finalize(frontend);
        Some(())
    }

    fn finish(&mut self, word: ir::Value) {
        self.builder.ins().store(flags(), word, self.result, 0);
        let status = self.builder.ins().iconst(types::I32, 0);
        self.builder.ins().return_(&[status]);
    }

    /// Atoms of index arithmetic nodes read as Numbers.
    fn integer_node_atoms(&self) -> Vec<Atom> {
        [&self.nest.prologue, &self.nest.body]
            .iter()
            .flat_map(|program| program.nodes.iter())
            .filter_map(|node| match node {
                Node::Integer(poly) => Some(poly.atoms().collect::<Vec<_>>()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// The entry bits of an invariant atom: its register's, or its capture's entry word.
    fn invariant_bits(&mut self, atom: Atom, entry: ir::Value) -> Option<ir::Value> {
        Some(match atom {
            Atom::Register(register) => {
                self.builder.ins().load(types::I64, flags(), self.registers, i32::from(register) * WORD_BYTES as i32)
            }
            Atom::Capture(slot) => {
                let index = self.region.captures.iter().position(|captured| *captured == slot)?;
                let word = (super::osr_loop::ENTRY_CAPTURES + index) as i32 * WORD_BYTES as i32;
                self.builder.ins().load(types::I64, flags(), entry, word)
            }
            _ => return None,
        })
    }

    /// An encoded integer within `MAX_INDEX_INPUT`, as an `i64`; anything else does not enter.
    fn unbox_integer(&mut self, bits: ir::Value) -> Option<ir::Value> {
        let tag = self.builder.ins().band_imm_u(bits, ValueLayout::TAG_MASK as i64);
        let other = self.builder.ins().icmp_imm_u(IntCC::NotEqual, tag, ValueLayout::INT_TAG as i64);
        self.fail_if(other, self.not_entered);
        let narrow = self.builder.ins().ireduce(types::I32, bits);
        let value = self.builder.ins().sextend(types::I64, narrow);
        let magnitude = self.builder.ins().iabs(value);
        let large = self.builder.ins().icmp_imm_s(IntCC::SignedGreaterThan, magnitude, MAX_INDEX_INPUT);
        self.fail_if(large, self.not_entered);
        Some(value)
    }

    fn fail_if(&mut self, failed: ir::Value, block: ir::Block) {
        let pass = self.builder.create_block();
        self.builder.ins().brif(failed, block, &[], pass, &[]);
        self.builder.switch_to_block(pass);
    }

    fn leave_if(&mut self, failed: ir::Value, leave: ir::Block, row: ir::Value) {
        let pass = self.builder.create_block();
        self.builder.ins().brif(failed, leave, &[ir::BlockArg::Value(row)], pass, &[]);
        self.builder.switch_to_block(pass);
    }

    /// `poly` at the row `row` and the element `column` (`Some(None)`: element zero; `None`
    /// for each: a polynomial of neither).
    fn evaluate(&mut self, poly: &Poly, row: Option<ir::Value>, column: Option<Option<ir::Value>>) -> ir::Value {
        let mut sum = self.builder.ins().iconst(types::I64, 0);
        for (monomial, coefficient) in &poly.0 {
            let mut term: Option<ir::Value> = None;
            let mut zero = false;
            for atom in monomial {
                let value = match atom {
                    Atom::Unit => continue,
                    Atom::Row => row.expect("a row polynomial evaluated at a row"),
                    Atom::Column => match column.expect("a column polynomial evaluated at a column") {
                        Some(column) => column,
                        None => {
                            zero = true;
                            continue;
                        }
                    },
                    other => self.integers[other],
                };
                term = Some(match term {
                    None => value,
                    Some(product) => self.builder.ins().imul(product, value),
                });
            }
            if zero {
                continue;
            }
            let term = match term {
                None => self.builder.ins().iconst(types::I64, *coefficient),
                Some(product) if *coefficient == 1 => product,
                Some(product) => self.builder.ins().imul_imm_s(product, *coefficient),
            };
            sum = self.builder.ins().iadd(sum, term);
        }
        sum
    }

    /// The address of an access's element at a row and element.
    fn address(&mut self, access: usize, row: ir::Value, column: ir::Value) -> ir::Value {
        let access = self.nest.accesses[access].clone();
        let index = self.evaluate(&access.index, Some(row), Some(Some(column)));
        let offset = self.builder.ins().imul_imm_s(index, WORD_BYTES);
        self.builder.ins().iadd(self.arrays[&access.array].elements, offset)
    }

    /// The save area's address of the element `column` of written array `slot` in the pair's
    /// row `offset`.
    fn saved(&mut self, area: ir::Value, slot: usize, offset: i64, column: ir::Value) -> ir::Value {
        let index = (slot as i64 * PAIR_ROWS + offset) * MAX_ROW_ELEMENTS;
        let saved_index = self.builder.ins().iadd_imm_s(column, index);
        let saved_offset = self.builder.ins().imul_imm_s(saved_index, WORD_BYTES);
        self.builder.ins().iadd(area, saved_offset)
    }

    /// Copies back the first `stored[slot * PAIR_ROWS + offset]` saved elements of each
    /// written array's row of the pair.
    fn restore_rows(&mut self, row: ir::Value, stored: &[ir::Value], area: ir::Value) {
        for (slot, (_, store)) in self.nest.stores.clone().into_iter().enumerate() {
            for offset in 0..PAIR_ROWS {
                let count = stored[slot * PAIR_ROWS as usize + offset as usize];
                let at_row = self.builder.ins().iadd_imm_s(row, offset);
                let block = self.builder.create_block();
                let body = self.builder.create_block();
                let after = self.builder.create_block();
                self.builder.append_block_param(block, types::I64);
                let zero = self.builder.ins().iconst(types::I64, 0);
                self.builder.ins().jump(block, &[ir::BlockArg::Value(zero)]);
                self.builder.switch_to_block(block);
                let column = self.builder.block_params(block)[0];
                let more = self.builder.ins().icmp(IntCC::SignedLessThan, column, count);
                self.builder.ins().brif(more, body, &[], after, &[]);
                self.builder.switch_to_block(body);
                let element = self.address(store, at_row, column);
                let saved = self.saved(area, slot, offset, column);
                let word = self.builder.ins().load(types::I64, flags(), saved, 0);
                self.builder.ins().store(flags(), word, element, 0);
                let next = self.builder.ins().iadd_imm_s(column, 1);
                self.builder.ins().jump(block, &[ir::BlockArg::Value(next)]);
                self.builder.switch_to_block(after);
            }
        }
    }

    /// Runs a row's prologue: the row values, as `f64`.
    fn run_prologue(&mut self, row: ir::Value, place: Place) -> Option<Vec<ir::Value>> {
        let program = self.nest.prologue.clone();
        let zero = self.builder.ins().iconst(types::I64, 0);
        let values = self.run(&program, row, zero, &[], place)?;
        Some(self.nest.rows.iter().map(|value| values[value.init]).collect())
    }

    /// The carried row values' current values.
    fn carried(&self, values: &[ir::Value]) -> Vec<ir::Value> {
        self.nest.rows.iter().zip(values).filter(|(value, _)| value.next.is_some()).map(|(_, value)| *value).collect()
    }

    /// The row values with the carried ones replaced by `carried`, in order.
    fn with_carried(&self, values: &[ir::Value], carried: &[ir::Value]) -> Vec<ir::Value> {
        let mut carried = carried.iter();
        self.nest
            .rows
            .iter()
            .zip(values)
            .map(|(value, current)| if value.next.is_some() { *carried.next().expect("a carried value") } else { *current })
            .collect()
    }

    /// Runs the body at a row's element, from the row values `values`: the carried values
    /// for the next element.
    fn run_element(&mut self, row: ir::Value, column: ir::Value, values: &[ir::Value], place: Place) -> Option<Vec<ir::Value>> {
        let program = self.nest.body.clone();
        let nodes = self.run(&program, row, column, values, place)?;
        Some(self.nest.rows.iter().filter_map(|value| value.next.map(|next| nodes[next])).collect())
    }

    /// Runs a program's steps at `place`; each node's `f64`.
    fn run(&mut self, program: &Program, row: ir::Value, column: ir::Value, values: &[ir::Value], place: Place) -> Option<Vec<ir::Value>> {
        let mut nodes: Vec<Option<ir::Value>> = vec![None; program.nodes.len()];
        // The written arrays this element has stored so far.
        let mut stored_here = vec![false; self.nest.stores.len()];
        for step in &program.steps {
            match step {
                Step::Node(index) => {
                    let value = match &program.nodes[*index] {
                        Node::Load(access) => {
                            let address = self.address(*access, row, column);
                            let bits = self.builder.ins().load(types::I64, flags(), address, 0);
                            // A failed read restores what the pair stored: this row's elements
                            // before this one and this one's stores so far, and the other row's.
                            let failed = self.builder.create_block();
                            self.builder.set_cold_block(failed);
                            let current = self.builder.current_block()?;
                            self.builder.switch_to_block(failed);
                            let mut counts = Vec::new();
                            for here in &stored_here {
                                for offset in 0..PAIR_ROWS {
                                    let count = if offset == place.row {
                                        self.builder.ins().iadd_imm_s(column, i64::from(*here))
                                    } else {
                                        place.other_stored
                                    };
                                    counts.push(ir::BlockArg::Value(count));
                                }
                            }
                            self.builder.ins().jump(place.pair.restore, &counts);
                            self.builder.switch_to_block(current);
                            unbox_number(&mut self.builder, bits, failed)
                        }
                        Node::Row(value) => *values.get(*value)?,
                        Node::Invariant(invariant) => self.numbers[invariant],
                        Node::Constant(number) => self.builder.ins().f64const(*number),
                        Node::Integer(poly) => {
                            let integer = self.evaluate(poly, Some(row), Some(Some(column)));
                            self.builder.ins().fcvt_from_sint(types::F64, integer)
                        }
                        Node::Arithmetic(operation, left, right) => {
                            let (left, right) = (nodes[*left]?, nodes[*right]?);
                            super::emit_number_arithmetic_operation(&mut self.builder, *operation, left, right)
                        }
                        Node::Unary(operation, value) => {
                            let value = nodes[*value]?;
                            super::emit_number_unary_operation(&mut self.builder, *operation, value)
                        }
                    };
                    nodes[*index] = Some(value);
                }
                Step::Store(access, value) => {
                    let address = self.address(*access, row, column);
                    let bits = box_number(&mut self.builder, nodes[*value]?);
                    // Save the element the store replaces.
                    let slot = self.nest.stores.iter().position(|(_, store)| store == access)?;
                    let old = self.builder.ins().load(types::I64, flags(), address, 0);
                    let saved = self.saved(place.pair.save, slot, place.row, column);
                    self.builder.ins().store(flags(), old, saved, 0);
                    self.builder.ins().store(flags(), bits, address, 0);
                    stored_here[slot] = true;
                }
            }
        }
        // Every node is a step, so each was computed.
        nodes.into_iter().collect()
    }
}

/// A pair's save area and restore block.
#[derive(Clone, Copy)]
struct Pair {
    save: ir::Value,
    restore: ir::Block,
}

/// Where a row program runs: the pair, the row of it (0 or 1), and the elements the other
/// row has stored.
#[derive(Clone, Copy)]
struct Place {
    pair: Pair,
    row: i64,
    other_stored: ir::Value,
}

fn flags() -> ir::MemFlagsData {
    ir::MemFlagsData::trusted()
}

/// The Number a value's bits encode, as an `f64`: a double, or an encoded integer; anything
/// else branches to `otherwise`. The test of `emit_js_tagged_number`, with its rare paths laid
/// out cold: in a row's element, every read takes it, and the doubles pass at its first test.
fn unbox_number(builder: &mut FunctionBuilder<'_>, bits: ir::Value, otherwise: ir::Block) -> ir::Value {
    let double = builder.ins().bitcast(types::F64, ir::MemFlagsData::new(), bits);
    let unordered = builder.ins().fcmp(FloatCC::Unordered, double, double);
    let nan_block = builder.create_block();
    builder.set_cold_block(nan_block);
    let tagged_block = builder.create_block();
    builder.set_cold_block(tagged_block);
    let integer_block = builder.create_block();
    builder.set_cold_block(integer_block);
    let join = builder.create_block();
    builder.append_block_param(join, types::F64);
    builder.ins().brif(unordered, nan_block, &[], join, &[ir::BlockArg::Value(double)]);
    builder.switch_to_block(nan_block);
    let field = builder.ins().ushr_imm_u(bits, i64::from(ValueLayout::TAG_SHIFT));
    let offset = builder.ins().iadd_imm_s(field, -(ValueLayout::FIRST_TAG_FIELD as i64));
    let tagged = builder.ins().icmp_imm_u(IntCC::UnsignedLessThanOrEqual, offset, ValueLayout::TAG_FIELD_SPAN as i64);
    builder.ins().brif(tagged, tagged_block, &[], join, &[ir::BlockArg::Value(double)]);
    builder.switch_to_block(tagged_block);
    let int_field = (ValueLayout::INT_TAG >> ValueLayout::TAG_SHIFT) as i64;
    let is_int = builder.ins().icmp_imm_u(IntCC::Equal, field, int_field);
    builder.ins().brif(is_int, integer_block, &[], otherwise, &[]);
    builder.switch_to_block(integer_block);
    let integer = builder.ins().ireduce(types::I32, bits);
    let number = builder.ins().fcvt_from_sint(types::F64, integer);
    builder.ins().jump(join, &[ir::BlockArg::Value(number)]);
    builder.switch_to_block(join);
    builder.block_params(join)[0]
}

/// `Value::number(number)`'s bits, as `emit_js_box_number` encodes them: an integer when the
/// truncation converts back to the same bits, the canonical NaN for any NaN, else the
/// double's bits. A fractional result, a stencil's usual one, takes a single test; the
/// integral path is laid out cold.
fn box_number(builder: &mut FunctionBuilder<'_>, number: ir::Value) -> ir::Value {
    let truncated = builder.ins().trunc(number);
    let integral = builder.ins().fcmp(FloatCC::UnorderedOrEqual, truncated, number);
    let fractional_block = builder.create_block();
    let integral_block = builder.create_block();
    builder.set_cold_block(integral_block);
    let join = builder.create_block();
    builder.append_block_param(join, types::I64);
    builder.ins().brif(integral, integral_block, &[], fractional_block, &[]);
    builder.switch_to_block(fractional_block);
    let fractional = builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number);
    builder.ins().jump(join, &[ir::BlockArg::Value(fractional)]);
    builder.switch_to_block(integral_block);
    let integer = builder.ins().fcvt_to_sint_sat(types::I32, number);
    let back = builder.ins().fcvt_from_sint(types::F64, integer);
    let back_bits = builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), back);
    let bits = builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number);
    let exact = builder.ins().icmp(IntCC::Equal, back_bits, bits);
    let payload = builder.ins().uextend(types::I64, integer);
    let encoded = builder.ins().bor_imm_u(payload, ValueLayout::INT_TAG as i64);
    let integral = builder.ins().select(exact, encoded, bits);
    let nan = builder.ins().fcmp(FloatCC::Unordered, number, number);
    let canonical = builder.ins().iconst(types::I64, ValueLayout::CANONICAL_NAN as i64);
    let integral = builder.ins().select(nan, canonical, integral);
    builder.ins().jump(join, &[ir::BlockArg::Value(integral)]);
    builder.switch_to_block(join);
    builder.block_params(join)[0]
}
