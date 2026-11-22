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
use crate::bytecode::{Function, Op, Successors, Traits, WideInstruction};
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

/// The families a boxed op and its raw form share (their catalog rows' family traits).
const RAW_FAMILIES: u128 = Traits::I64_BINARY_FAMILY
    .union(Traits::I64_BINARY_IMMEDIATE_FAMILY)
    .union(Traits::I64_UNARY_FAMILY)
    .union(Traits::F64_BINARY_FAMILY)
    .union(Traits::F64_UNARY_FAMILY)
    .union(Traits::RETURN_FAMILY)
    .union(Traits::STRUCT_GET_FAMILY)
    .union(Traits::STRUCT_SET_FAMILY)
    .union(Traits::ARRAY_GET_FAMILY)
    .union(Traits::ARRAY_SET_FAMILY)
    .0;

/// The raw form of `op`: the opcode that runs the same operation over raw i64 registers, the
/// raw row of `op`'s family. A raw op has none.
pub(crate) fn raw_form(op: Op) -> Option<Op> {
    RAW_FORMS[op as usize]
}

/// `raw_form_scan` of every op, tabled once at compile time.
const RAW_FORMS: [Option<Op>; Op::COUNT] = {
    let mut table = [None; Op::COUNT];
    let mut index = 0;
    while index < Op::COUNT {
        if let Some(op) = Op::from_index(index) {
            table[index] = raw_form_scan(op);
        }
        index += 1;
    }
    table
};

/// The raw row of `op`'s family, found by scanning the catalog.
const fn raw_form_scan(op: Op) -> Option<Op> {
    if is_raw(op) {
        return None;
    }
    let family = op.traits().0 & RAW_FAMILIES;
    if family == 0 {
        return None;
    }
    let mut index = 0;
    while index < Op::COUNT {
        if let Some(candidate) = Op::from_index(index)
            && is_raw(candidate)
            && candidate.traits().0 & RAW_FAMILIES == family
        {
            return Some(candidate);
        }
        index += 1;
    }
    None
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
    op.traits().has(Traits::RAW_FORM)
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
    if let Some(roles) = field_roles(instruction, rewritten) {
        return Some(roles);
    }
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
    let traits = op.traits();
    if traits.has(Traits::REGISTER_COPY) {
        Some(Roles::Copy { destination: a, sources: vec![b] })
    } else if traits.has(Traits::WASM_SELECT) {
        Some(Roles::Copy { destination: a, sources: vec![b, instruction.c()] })
    } else if (traits.has(Traits::CONSTANT_LOAD) || traits.has(Traits::REGISTER_FILL))
        && (rewritten || constant_neutral(instruction.constant_index()))
    {
        Some(Roles::Neutral)
    } else if traits.has(Traits::RETURN_VALUE) && returns_wide && !rewritten {
        Some(Roles::Wide { uses: vec![a], definitions: vec![], neutral_uses: vec![] })
    } else if traits.has(Traits::RETURN_FAMILY) && traits.has(Traits::RAW_FORM) {
        Some(Roles::Wide { uses: vec![a], definitions: vec![], neutral_uses: vec![] })
    } else {
        None
    }
}

/// The roles of a GC field access of an i64 or f64 field (`GcFieldValue::Wide`, `struct.get`,
/// `struct.set`, `array.get`, `array.set`): its data register, a read's result or a write's
/// stored value, is wide, since the field's static type says its bits are an i64's or an f64's
/// and the access moves them between the register and the storage untouched. The reference and
/// the array index stay boxed. `None` for any other instruction or field class.
fn field_roles(instruction: WideInstruction, rewritten: bool) -> Option<Roles> {
    use crate::staged::wasm_gc::{Aggregate, GcStep};
    use crate::wasm::gc::{GcField, GcFieldAccess, GcFieldValue};
    let op = instruction.op();
    if !(if rewritten { is_raw(op) } else { raw_form(op).is_some() || is_raw(op) }) {
        return None;
    }
    let (aggregate, access) = GcStep::of(op)?.field()?;
    if GcField::from_tag(instruction.imm())?.value() != GcFieldValue::Wide {
        return None;
    }
    let write = access == GcFieldAccess::Write;
    let data = match (aggregate, write) {
        (Aggregate::Struct, true) => instruction.b(),
        (Aggregate::Array, true) => instruction.c(),
        (_, false) => instruction.a(),
    };
    Some(if write {
        Roles::Wide { uses: vec![data], definitions: vec![], neutral_uses: vec![] }
    } else {
        Roles::Wide { uses: vec![], definitions: vec![data], neutral_uses: vec![] }
    })
}

/// Union-find over value nodes.
/// The web classes: the shared union-find (`crate::webs::DefSets`), and per root whether the
/// class is touched boxed.
struct Classes {
    sets: crate::webs::DefSets,
    boxed: Vec<bool>,
}

impl Classes {
    fn add(&mut self) -> u32 {
        self.boxed.push(false);
        self.sets.add() as u32
    }

    fn len(&self) -> usize {
        self.sets.len()
    }

    fn union(&mut self, left: u32, right: u32) {
        if let Some((kept, absorbed)) = self.sets.union(right as usize, left as usize) {
            self.boxed[kept] |= self.boxed[absorbed];
        }
    }

    fn mark_boxed(&mut self, node: u32) {
        let root = self.sets.root(node as usize);
        self.boxed[root] = true;
    }

    fn is_boxed(&mut self, node: u32) -> bool {
        let root = self.sets.root(node as usize);
        self.boxed[root]
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
    let mut classes = Classes { sets: crate::webs::DefSets::default(), boxed: Vec::new() };
    // The node of each live-in register at each pc.
    let mut live_in: Vec<Vec<Option<u32>>> = Vec::with_capacity(count);
    for set in &live[..count] {
        let row = (0..registers).map(|register| set.contains(register).then(|| classes.add())).collect();
        live_in.push(row);
        if classes.len() > NODE_BUDGET {
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
/// liveness budget or the root-map width keeps every op boxed. Returns the live sets it solved,
/// which are the live sets of the function as it stands after the switch: a raw form has its
/// boxed op's transfers and control flow, so only the opcodes change. `None` where it solved none.
pub(crate) fn use_raw_forms(function: &mut Function, constant_neutral: &dyn Fn(usize) -> bool, returns_wide: bool) -> Option<Vec<RegisterSet>> {
    let no_method_sites: &[MethodSite] = &[];
    let transfers = super::transfers(function, no_method_sites, &[], &[])?;
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
            crate::bytecode::wide_entry_mut(&mut function.wide, packed).set_op(raw_op);
        } else {
            function.code[pc].set_op(raw_op);
        }
    }
    Some(live)
}

#[cfg(test)]
#[path = "raw_webs_tests.rs"]
mod tests;
