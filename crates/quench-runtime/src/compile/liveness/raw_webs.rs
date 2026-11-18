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
//! - *wide*: an i64/f64 operand or result of an op that has a raw form (`raw_capable`). All wide
//!   operands of one instruction share a web class, so its raw form reads and writes raw words only;
//! - *copy*: a move or select, which carries bits without reading them: its result joins its sources;
//! - *boxed*: everything else, including the entry's parameters and the initial registers.
//!
//! A web touched in the boxed role holds `Value`s; every other web holds raw words. The
//! collector scans a register at a pc only where it is live and its web there is boxed
//! (`RawWebs::raw_live`), and an instruction switches to its raw form where its wide web is raw
//! (`RawWebs::raw_ops`).

use super::RegisterSet;
use crate::bytecode::{Class, Function, Op, Successors, WideInstruction};
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

/// How an instruction touches its registers, where it is not all boxed.
enum Roles {
    /// A raw-capable op: these registers are its wide operands (uses) and result (definitions).
    Wide { uses: Vec<u16>, definitions: Vec<u16> },
    /// A copy of `sources` into `destination`.
    Copy { destination: u16, sources: Vec<u16> },
}

/// The roles of `instruction`, or `None` where it touches every register boxed. `constant_wide`
/// says a constant-pool entry is an i64/f64 scalar; `returns_wide` that the function's result is.
fn roles(instruction: WideInstruction, constant_wide: &dyn Fn(usize) -> bool, returns_wide: bool) -> Option<Roles> {
    let op = instruction.op();
    let (a, b) = (instruction.a(), instruction.b());
    if let Some(operation) = WasmOperation::of(op, instruction.imm()) {
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
        return (!uses.is_empty() || !definitions.is_empty()).then_some(Roles::Wide { uses, definitions });
    }
    match op.class() {
        Class::Move => Some(Roles::Copy { destination: a, sources: vec![b] }),
        Class::WasmSelect => Some(Roles::Copy { destination: a, sources: vec![b, instruction.c()] }),
        Class::LoadConst if constant_wide(instruction.constant_index()) => {
            Some(Roles::Wide { uses: vec![], definitions: vec![a] })
        }
        Class::Return if returns_wide && op == Op::Return => Some(Roles::Wide { uses: vec![a], definitions: vec![] }),
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
    constant_wide: &dyn Fn(usize) -> bool,
    returns_wide: bool,
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
        let roles = roles(*instruction, constant_wide, returns_wide);
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
            Some(Roles::Wide { uses: wide_uses, definitions: wide_definitions }) => {
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

#[cfg(test)]
#[path = "raw_webs_tests.rs"]
mod tests;
