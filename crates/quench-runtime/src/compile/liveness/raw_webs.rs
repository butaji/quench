//! Which Wasm values may live in their registers as raw machine words (an i64 or f64's bits)
//! rather than as `Value`s: the interpreter then runs an i64 or f64 op as its C analogue plus
//! dispatch and decode, with no tag checks and no boxes (`raw-registers-design.md`).
//!
//! The unit is the *web*: a register's definitions and uses connected through the values that
//! flow between them, as register allocators build them. The lowering allocates operand-stack
//! registers by depth, so one register is an i64 temporary in one place and a reference in
//! another; its webs are separate. Every instruction touches each register it reads or writes
//! (`uses`, `definitions`: generated from the opcode table, never listed here) in one of three
//! roles:
//! - *wide*: an i64 operand or result of an op that has a raw form (`raw_form`). All wide
//!   operands of one instruction share a web class, so its raw form reads and writes raw words only;
//! - *copy*: a move or select, which carries bits without reading them: its result joins its sources;
//! - *neutral*: a constant load or fill whose `Value` is the constant's own word, which a raw and
//!   a boxed register hold alike: it joins whatever web its register's uses decide;
//! - *boxed*: everything else, including the entry's parameters and the initial registers.
//!
//! A raw word a tag claims would read as a reference; the collector reads registers through
//! liveness masks and marks a handle only where it names an occupied record (bounds- and
//! occupancy-checked, as for a stale word above a window's fill), so such a word at worst keeps
//! a record alive until its register is overwritten.
//!
//! A web touched in the boxed role holds `Value`s; every other web holds raw words. The
//! collector scans a register at a pc only where it is live and its web there is boxed
//! (`RawWebs::raw_live`), and an instruction switches to its raw form where its wide web is raw
//! (`RawWebs::raw_ops`).

use super::RegisterSet;
use crate::bytecode::{Class, Function, Op, Successors, WideInstruction};
use crate::compile::liveness::MethodSite;
use crate::staged::wasm_operation::{Operand, WasmOperation};
use crate::WasmType;

/// The raw webs of a function.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct RawWebs {
    /// Per pc, whether the instruction runs its raw form.
    pub(crate) raw_ops: Vec<bool>,
    /// Per pc, the registers live on entry whose value there is a raw word.
    pub(crate) raw_live: Vec<RegisterSet>,
}

const fn wide(ty: WasmType) -> bool {
    matches!(ty, WasmType::I64 | WasmType::F64)
}

/// The raw form of `op`: the opcode that runs the same operation over raw i64 registers.
pub(crate) const fn raw_form(op: Op) -> Option<Op> {
    Some(match op {
        Op::WasmI64Binary => Op::WasmI64BinaryRaw,
        Op::WasmI64BinaryImmediate => Op::WasmI64BinaryImmediateRaw,
        Op::WasmI64Unary => Op::WasmI64UnaryRaw,
        Op::WasmF64Binary => Op::WasmF64BinaryRaw,
        Op::WasmF64Unary => Op::WasmF64UnaryRaw,
        Op::Return => Op::WasmReturn64,
        _ => return None,
    })
}

/// Whether `operation` is an f64 operator whose result depends on a NaN operand only through
/// its being a NaN (Wasm leaves such a result's payload open): its raw and boxed forms read any
/// word of an f64 register.
fn nan_agnostic(operation: WasmOperation) -> bool {
    use crate::staged::wasm_float::Float;
    match operation {
        WasmOperation::FloatBinary { operator, float: Float::F64 } => operator.nan_agnostic(),
        WasmOperation::FloatUnary { operator, float: Float::F64 } => operator.nan_agnostic(),
        _ => false,
    }
}

/// Whether `op` is a raw form: analyzing code the lowering already switched finds its webs again.
const fn is_raw(op: Op) -> bool {
    matches!(op, Op::WasmI64BinaryRaw | Op::WasmI64BinaryImmediateRaw | Op::WasmI64UnaryRaw | Op::WasmF64BinaryRaw | Op::WasmF64UnaryRaw | Op::WasmReturn64)
}

/// How an instruction touches its registers, where it is not all boxed.
enum Roles {
    /// A raw-capable op: these registers are its wide operands (uses) and result (definitions).
    Wide { uses: Vec<u16>, definitions: Vec<u16>, neutral_uses: Vec<u16> },
    /// A copy of `sources` into `destination`.
    Copy { destination: u16, sources: Vec<u16> },
    /// Definitions whose word reads the same raw and boxed.
    Neutral,
}

/// The roles of `instruction`, or `None` where it touches every register boxed. `constant_neutral`
/// says a constant-pool entry's `Value` is its own word (`Value::from_bits64`); `returns_wide`
/// that the function's result is an i64.
/// With `rewritten`, of code the lowering already switched: only raw forms are wide (an op
/// it left boxed is boxed), and every constant load and fill is neutral, since a web holding a
/// constant whose `Value` is not its word was boxed when the lowering decided.
fn roles(instruction: WideInstruction, constant_neutral: &dyn Fn(usize) -> bool, returns_wide: bool, rewritten: bool) -> Option<Roles> {
    let op = instruction.op();
    let (a, b) = (instruction.a(), instruction.b());
    if (if rewritten { is_raw(op) } else { raw_form(op).is_some() || is_raw(op) })
        && let Some(operation) = WasmOperation::of(op, instruction.imm())
    {
        let operand_wide = wide(operation.operand_type());
        let result_wide = wide(operation.result_type());
        let mut uses = Vec::new();
        let definitions = if result_wide { vec![a] } else { vec![] };
        match operation {
            WasmOperation::Binary { form, .. } if !form.branches() => {
                if operand_wide {
                    uses.push(form.left(instruction));
                    if let Operand::Register(right) = form.right(instruction) {
                        uses.push(right);
                    }
                }
            }
            // A float operator that treats a NaN operand only as a NaN reads a raw word and a
            // `Value` alike (a boxed payload's bits are a NaN, `Value::float_bits64`): its
            // operands join no web and box none.
            operation if nan_agnostic(operation) => {
                let read = match operation {
                    WasmOperation::FloatBinary { .. } => vec![b, instruction.c()],
                    _ => vec![b],
                };
                return Some(Roles::Wide { uses, definitions, neutral_uses: read });
            }
            WasmOperation::Unary { .. } | WasmOperation::FloatUnary { .. } | WasmOperation::Convert { .. } => {
                if operand_wide {
                    uses.push(b);
                }
            }
            WasmOperation::FloatBinary { .. } => {
                if operand_wide {
                    uses.extend([b, instruction.c()]);
                }
            }
            _ => return None,
        }
        return (!uses.is_empty() || !definitions.is_empty()).then_some(Roles::Wide { uses, definitions, neutral_uses: vec![] });
    }
    match op.class() {
        Class::Move => Some(Roles::Copy { destination: a, sources: vec![b] }),
        Class::WasmSelect => Some(Roles::Copy { destination: a, sources: vec![b, instruction.c()] }),
        Class::LoadConst | Class::WasmFillRegisters if rewritten || constant_neutral(instruction.constant_index()) => Some(Roles::Neutral),
        Class::Return if returns_wide && !rewritten && op == Op::Return => Some(Roles::Wide { uses: vec![a], definitions: vec![], neutral_uses: vec![] }),
        Class::WasmReturn64 => Some(Roles::Wide { uses: vec![a], definitions: vec![], neutral_uses: vec![] }),
        _ => None,
    }
}

/// Union-find over value nodes.
struct Classes {
    parent: Vec<u32>,
    boxed: Vec<bool>,
}

impl Classes {
    fn add(&mut self) -> u32 {
        let id = self.parent.len() as u32;
        self.parent.push(id);
        self.boxed.push(false);
        id
    }

    fn find(&mut self, mut node: u32) -> u32 {
        while self.parent[node as usize] != node {
            let parent = self.parent[node as usize];
            self.parent[node as usize] = self.parent[parent as usize];
            node = parent;
        }
        node
    }

    fn union(&mut self, left: u32, right: u32) {
        let (left, right) = (self.find(left), self.find(right));
        if left != right {
            self.parent[left as usize] = right;
            self.boxed[right as usize] |= self.boxed[left as usize];
        }
    }

    fn mark_boxed(&mut self, node: u32) {
        let root = self.find(node);
        self.boxed[root as usize] = true;
    }

    fn is_boxed(&mut self, node: u32) -> bool {
        let root = self.find(node);
        self.boxed[root as usize]
    }
}

/// Live-in nodes beyond this many stay unanalyzed: such a function keeps every web boxed.
const NODE_BUDGET: usize = 1 << 22;

/// The raw webs of the Wasm function `function`, whose registers live on entry to each pc are
/// `live` (one entry per pc, from the liveness pass) and whose instructions touch `transfers`
/// (uses, definitions). A function with exception handlers, or past the node budget, has none.
pub(crate) fn raw_webs(
    function: &Function,
    live: &[RegisterSet],
    transfers: &[(RegisterSet, RegisterSet)],
    constant_neutral: &dyn Fn(usize) -> bool,
    returns_wide: bool,
) -> RawWebs {
    webs(function, live, transfers, constant_neutral, returns_wide, false)
}

/// The registers live before each pc of the lowered Wasm function `function` that hold raw
/// words there, which a collection must not read as `Value`s; `None` where it has no raw op.
pub(crate) fn raw_live(function: &Function) -> Option<Vec<RegisterSet>> {
    if !function.instructions().any(|instruction| is_raw(instruction.op())) {
        return None;
    }
    let no_method_sites: &[MethodSite] = &[];
    let transfers = super::transfers(function, no_method_sites, &[], &[])?;
    let live = super::solve::analyze_wide(function, &transfers, &Successors::of_code(function));
    Some(webs(function, &live, &transfers, &|_| true, false, true).raw_live)
}

fn webs(
    function: &Function,
    live: &[RegisterSet],
    transfers: &[(RegisterSet, RegisterSet)],
    constant_neutral: &dyn Fn(usize) -> bool,
    returns_wide: bool,
    rewritten: bool,
) -> RawWebs {
    let instructions: Vec<WideInstruction> = function.instructions().collect();
    let count = instructions.len();
    let registers = usize::from(function.registers);
    if !function.handlers.is_empty() || live.len() < count || transfers.len() < count {
        return RawWebs::default();
    }
    let mut classes = Classes { parent: Vec::new(), boxed: Vec::new() };
    // The node of each live-in register at each pc.
    let mut live_in: Vec<Vec<Option<u32>>> = Vec::with_capacity(count);
    for set in &live[..count] {
        let row = (0..registers).map(|register| set.contains(register).then(|| classes.add())).collect();
        live_in.push(row);
        if classes.parent.len() > NODE_BUDGET {
            return RawWebs::default();
        }
    }
    // The entry writes Values: parameters and initial registers.
    if let Some(entry) = live_in.first() {
        for node in entry.iter().flatten() {
            classes.mark_boxed(*node);
        }
    }
    let successors = Successors::of_code(function);
    let mut wide_group: Vec<Option<u32>> = vec![None; count];
    for (pc, instruction) in instructions.iter().enumerate() {
        let (uses, definitions) = transfers[pc];
        let roles = roles(*instruction, constant_neutral, returns_wide, rewritten);
        // Each definition is a node, flowing into the successors where its register is live.
        let mut defined = vec![None; registers];
        for register in (0..registers).filter(|register| definitions.contains(*register)) {
            let node = classes.add();
            defined[register] = Some(node);
            for (successor, _) in successors.at(pc) {
                if let Some(Some(next)) = live_in.get(successor).map(|row| row[register]) {
                    classes.union(node, next);
                }
            }
        }
        // A register live across the instruction, not defined by it, carries its value on.
        for register in 0..registers {
            if defined[register].is_some() {
                continue;
            }
            let Some(here) = live_in[pc][register] else { continue };
            for (successor, _) in successors.at(pc) {
                if let Some(Some(next)) = live_in.get(successor).map(|row| row[register]) {
                    classes.union(here, next);
                }
            }
        }
        let use_node = |classes: &mut Classes, register: u16| -> u32 {
            // A use of a register not live on entry reads a value no definition reached: a fresh node.
            live_in[pc].get(usize::from(register)).copied().flatten().unwrap_or_else(|| classes.add())
        };
        let mut covered_uses = RegisterSet::default();
        let mut covered_definitions = RegisterSet::default();
        match &roles {
            Some(Roles::Wide { uses: wide_uses, definitions: wide_definitions, neutral_uses }) => {
                for &register in neutral_uses {
                    covered_uses.insert(usize::from(register));
                }
                let group = classes.add();
                for &register in wide_uses {
                    let node = use_node(&mut classes, register);
                    classes.union(node, group);
                    covered_uses.insert(usize::from(register));
                }
                for &register in wide_definitions {
                    if let Some(node) = defined.get(usize::from(register)).copied().flatten() {
                        classes.union(node, group);
                    }
                    covered_definitions.insert(usize::from(register));
                }
                wide_group[pc] = Some(group);
            }
            Some(Roles::Copy { destination, sources }) => {
                if let Some(node) = defined.get(usize::from(*destination)).copied().flatten() {
                    for &source in sources {
                        let from = use_node(&mut classes, source);
                        classes.union(node, from);
                        covered_uses.insert(usize::from(source));
                    }
                    covered_definitions.insert(usize::from(*destination));
                }
            }
            Some(Roles::Neutral) => covered_definitions = definitions,
            None => {}
        }
        // Every other register the instruction touches, it touches boxed.
        for register in 0..registers {
            if uses.contains(register) && !covered_uses.contains(register) {
                let node = use_node(&mut classes, register as u16);
                classes.mark_boxed(node);
            }
            if definitions.contains(register) && !covered_definitions.contains(register) {
                if let Some(node) = defined[register] {
                    classes.mark_boxed(node);
                }
            }
        }
    }
    let raw_ops = wide_group.iter().map(|group| group.is_some_and(|group| !classes.is_boxed(group))).collect();
    let raw_live = live_in
        .iter()
        .map(|row| {
            let mut set = RegisterSet::default();
            for (register, node) in row.iter().enumerate() {
                if let Some(node) = node
                    && !classes.is_boxed(*node)
                {
                    set.insert(register);
                }
            }
            set
        })
        .collect();
    RawWebs { raw_ops, raw_live }
}

/// Switches each op of the Wasm function `function` whose wide web is raw to its raw form
/// (`raw_form`). `constant_neutral` and `returns_wide` are as `raw_webs`'. A function past the
/// liveness budget or the root-map width keeps every op boxed.
pub(crate) fn use_raw_forms(function: &mut Function, constant_neutral: &dyn Fn(usize) -> bool, returns_wide: bool) {
    let no_method_sites: &[MethodSite] = &[];
    let Some(transfers) = super::transfers(function, no_method_sites, &[], &[]) else { return };
    let live = super::solve::analyze_wide(function, &transfers, &Successors::of_code(function));
    let webs = raw_webs(function, &live, &transfers, constant_neutral, returns_wide);
    for (pc, raw) in webs.raw_ops.iter().enumerate() {
        if !raw {
            continue;
        }
        let packed = function.code[pc];
        let op = function.instruction_at(pc).op();
        let Some(raw_op) = raw_form(op) else { continue };
        if packed.is_wide() {
            function.wide[packed.wide_index()].set_op(raw_op);
        } else {
            function.code[pc].set_op(raw_op);
        }
    }
}

#[cfg(test)]
#[path = "raw_webs_tests.rs"]
mod tests;
