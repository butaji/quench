//! Copy propagation over the bytecode: a read of a `Move`'s destination takes its source.
use super::*;
use crate::bytecode::{FieldLayout, ImmediateLayout, InstructionField, WideInstruction};
use crate::compile::CompileScratch;

#[cfg(test)]
#[path = "copies_tests.rs"]
mod tests;

/// Lets the reads of a `Move`'s destination read the move's source instead, through the rest of
/// the move's basic block: `d = Move s; …; x(d)` becomes `Nop; …; x(s)`. Control reaches each
/// reader only through the move, by instructions that fall through without writing `s` or `d`
/// (`scan_move`), so `s` still holds the value there. When no path reads `d` after the scan, the
/// move is dead and becomes a `Nop` too. A parameter's register is never a destination (legacy
/// `f.arguments` reflection reads it). Inside a handler region the move's reads are rewritten
/// only when no covering handler reads `d` (`CodeLiveness::handler_live`): an abrupt completion
/// in the span enters that handler with `s` and `d` as the span left them, so only `d` can
/// differ. Only register, operand and field-base fields are rewritten; a read through a window
/// or a register pair is never rewritten (`forward_read`). A read of a binding a statement
/// copied before evaluating its other operands (`o.v = o.v + 1`) then reads the binding: each
/// tier sees one value, not a copy.
pub(super) fn forward_move_sources(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
    current: &mut Option<CodeFacts>,
    scratch: &mut CompileScratch,
) -> bool {
    if !function.code.iter().any(|instruction| instruction.op() == MOVE) {
        return false;
    }
    let Some(mut analysis) = CodeLiveness::of(current, function, methods, field_sites, superinstructions, scratch) else {
        return false;
    };
    let scope = Scope::of(function);
    // The changed pcs, the readers of one move, and the readers already forwarded, all taken
    // from `scratch` and given back below.
    let mut changed = std::mem::take(&mut scratch.changed);
    let mut readers = std::mem::take(&mut scratch.readers);
    let mut rewritten_readers = std::mem::take(&mut scratch.rewritten_readers);
    changed.clear();
    rewritten_readers.reset(function.code.len());
    for pc in 0..function.code.len() {
        if rewritten_readers.contains(pc) {
            continue;
        }
        let uses = |instruction: WideInstruction| liveness::uses(instruction, methods, field_sites, superinstructions);
        let definitions = |instruction: WideInstruction| liveness::definitions(instruction, superinstructions);
        let Some(forwarding) = scan_move(function, &analysis, &scope, pc, (&uses, &definitions), &mut readers) else {
            continue;
        };
        for &(at, rewritten) in &readers {
            function.code[at] = rewritten;
            rewritten_readers.insert(at);
            changed.push(at);
        }
        // Why the live sets this scan leaves stale are safe for the next scans. Each rewritten
        // reader now reads the source where it read the destination, so the source is live from
        // the move through the last reader: the loop adds it over that range, the only bit a
        // rewrite could leave under-approximated. The destination is only over-approximated
        // there, since no reader reads it any more and keeping it live is conservative. Nothing
        // before the move changes: the move read the source and wrote the destination itself.
        // Each later scan takes its masks from the code as it now stands (`liveness::uses` and
        // `definitions` of each instruction), so a rewritten reader or a dropped move is seen at
        // once, and `hand_on` re-derives the transfer masks of the changed positions.
        if let Some(&(last, _)) = readers.last() {
            for live in &mut analysis.live[pc + 1..=last] {
                *live |= forwarding.source;
            }
        }
        if forwarding.dead && !readers.is_empty() {
            function.code[pc] = Instr::new(NOP, 0, 0, 0, 0);
            changed.push(pc);
        }
    }
    scratch.readers = readers;
    scratch.rewritten_readers = rewritten_readers;
    changed.sort_unstable();
    changed.dedup();
    let rewrote = hand_on(analysis, &changed, function, methods, field_sites, superinstructions, current);
    scratch.changed = changed;
    rewrote
}

/// What copy propagation may not cross or rewrite in a function.
struct Scope {
    protected: crate::bitset::BitSet,
    /// The parameters' promoted storage.
    parameters: RegisterSet,
}

impl Scope {
    fn of(function: &BcFunction) -> Self {
        let parameters = function.local_registers.iter()
            .filter(|entry| entry.local < function.params)
            .fold(RegisterSet::default(), |mask, entry| mask | register_bit(entry.register));
        Self {
            protected: protected_positions(&function.code, &function.wide, &function.handlers,
                function.parameter_end_pc, &function.binding_sites),
            parameters,
        }
    }

    /// Whether the instruction is a plain `Move` of one register to another that is no
    /// parameter's storage. A promoted local's register may take it: the move's destination is
    /// dead after its one reader, so the write is unobservable, as `remove_dead_moves` drops
    /// such a write; this is the per-iteration copy of `for (let i ...)` that its body reads
    /// (`a + i` then reads the loop's `i`). Only a parameter's register is reflected
    /// (`f.arguments`).
    fn forwards(&self, mov: Instr) -> bool {
        mov.op() == MOVE && !mov.is_wide()
            && register_bit(mov.result_register()) & self.parameters == 0 && mov.register_b() != mov.result_register()
    }
}

impl CodeLiveness {
    /// The registers the handlers covering `pc` read on entry: their targets' live sets.
    ///
    /// A rewrite may change which of two registers holds a value inside a span when no handler
    /// can observe either: an abrupt completion inside the span enters these handlers with every
    /// register as the span left it. A span never crosses a protected position, which includes
    /// each handler's start, end and target, so every instruction of a span is covered by the
    /// same handlers as its first one, and this set answers for all of them.
    pub(super) fn handler_live(&self, pc: usize) -> RegisterSet {
        self.successors
            .at(pc)
            .filter(|&(_, edge)| edge == crate::bytecode::ControlEdge::Handler)
            .fold(RegisterSet::default(), |live, (target, _)| live | self.live.get(target).copied().unwrap_or_default())
    }
}

/// The read and write masks of an instruction, taken of its widened form: a packed wide
/// instruction is only a reference into the function's wide table (`BcFunction::widened`).
type Masks<'m> = (&'m dyn Fn(WideInstruction) -> RegisterSet, &'m dyn Fn(WideInstruction) -> RegisterSet);

/// What forwarding one `Move` does: its source, and whether its destination is dead once the
/// scan ends. The readers its value reaches, each rewritten to read the source, are in the
/// scan's `readers`.
struct Forwarding {
    source: RegisterSet,
    dead: bool,
}

/// The scan of the `Move` at `pc`, from the instruction after it through the end of its basic
/// block, or to the first redefinition of its source or destination. Each read of the
/// destination on the way is rewritten to read the source (`forward_read`). The scan stops at a
/// read that cannot be rewritten, at a protected position (a join or region boundary, where the
/// move's value may meet other values), and at an instruction that leaves the block. `dead`
/// says the destination is not read after the scan on any path the move's value reaches.
/// The most live instructions one Move's scan looks at.
const SCAN_LIMIT: usize = 64;

fn scan_move(
    function: &BcFunction,
    analysis: &CodeLiveness,
    scope: &Scope,
    pc: usize,
    (uses, definitions): Masks<'_>,
    readers: &mut Vec<(usize, Instr)>,
) -> Option<Forwarding> {
    let mov = function.code[pc];
    if !scope.forwards(mov) {
        return None;
    }
    let (source, destination) = (mov.register_b(), mov.result_register());
    let (source_bit, destination_bit) = (register_bit(source), register_bit(destination));
    if analysis.handler_live(pc) & destination_bit != 0 {
        return None;
    }
    // Legacy `f.arguments` reflection may rewrite a parameter during the instructions between, so
    // a parameter's source forwards to its first live reader only.
    let parameter_source = source_bit & scope.parameters != 0;
    let dead_before = |at: usize| analysis.live[at] & destination_bit == 0;
    let dead_after = |at: usize| liveness::live_out(&analysis.successors, &analysis.live, at) & destination_bit == 0;
    readers.clear();
    let mut live_seen = 0;
    let mut dead = false;
    for at in pc + 1..function.code.len() {
        if scope.protected.contains(at) {
            dead = dead_before(at);
            break;
        }
        let instruction = function.code[at];
        if instruction.op() == NOP {
            continue;
        }
        if parameter_source && live_seen == 1 {
            dead = dead_before(at);
            break;
        }
        live_seen += 1;
        // Bounds the scan so that a long straight-line block costs linear load time; the Move
        // stays, and the readers forwarded so far are still valid.
        if live_seen > SCAN_LIMIT {
            break;
        }
        let widened = function.widened(instruction);
        let (read, written) = (uses(widened), definitions(widened));
        if read & destination_bit != 0 {
            let Some(rewritten) = forward_read(instruction, destination, source, (uses, definitions)) else {
                break;
            };
            readers.push((at, rewritten));
        }
        if !function.widened(instruction).op().control_flow_layout().only_falls_through() {
            dead = dead_after(at);
            break;
        }
        if written & destination_bit != 0 {
            dead = true;
            break;
        }
        if written & source_bit != 0 {
            dead = dead_after(at);
            break;
        }
    }
    Some(Forwarding { source: source_bit, dead })
}

/// `instruction` with its reads of `destination` moved to `source`, when that is one rewrite of
/// single-register fields: `None` for a wide instruction, a window or a register pair, or when
/// the rewritten read and definition masks are not exactly the original ones with that change.
fn forward_read(instruction: Instr, destination: Register, source: Register, (uses, definitions): Masks<'_>)
    -> Option<Instr> {
    if instruction.is_wide() || reads_window(instruction.op()) {
        return None;
    }
    let rewritten = substitute_register_read(instruction, destination, source)?;
    let (original, changed) = (instruction.as_wide(), rewritten.as_wide());
    let expected = (uses(original) & !register_bit(destination)) | register_bit(source);
    (uses(changed) == expected && definitions(changed) == definitions(original)).then_some(rewritten)
}

/// Whether `op` reads a register window or a register pair, which no single register replaces.
/// A `ConstructArguments` field is treated the same way, since it may read a window.
fn reads_window(op: Op) -> bool {
    matches!(
        op.immediate_layout(),
        ImmediateLayout::CallWindow
            | ImmediateLayout::CallWindowWithEvalFlags
            | ImmediateLayout::SingleArgumentCallWindowWithEvalFlags
            | ImmediateLayout::RegisterPair
    ) || InstructionField::ALL
        .iter()
        .any(|&field| matches!(op.field_layout(field), FieldLayout::RegisterWindowBase | FieldLayout::ConstructArguments))
}
