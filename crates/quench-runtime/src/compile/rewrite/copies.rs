//! Copy propagation over the bytecode: a read of a `Move`'s destination takes its source.
//!
//! `d = Move s; …; x(d); …; y(d)` becomes `Nop; …; x(s); …; y(s)`, the forward counterpart of
//! `coalesce_distant_producer_moves`. Control reaches the readers only through the move, by
//! instructions that fall through without writing `s` or `d` (`move_readers`); the moved
//! value is dead where that span ends, `d` is not promoted local storage (whose writes legacy
//! `f.arguments` reflection reads), and no handler the move's region enters reads `d`
//! (`CodeLiveness::handler_live`). Only register, operand and field-base fields are
//! rewritten; each reader's use and definition masks confirm the rewrite trades `d` for `s`
//! and nothing else. A read of a binding a statement copied before evaluating its other
//! operands (`o.v = o.v + 1`, `a[i] += x`, which reads `a` and `i` twice) then reads the
//! binding: each tier sees one value, not a copy.
//!
//! Each rewrite updates the transfer masks of its readers and of the dropped move at once, so
//! a later move feeding the same reader forwards too: `t = Move i; u = Move n; Lt t, u`
//! becomes `Lt i, n`, the two-operand reads of loop bounds and element accesses (`a[i]`,
//! `i < n`). The live sets a rewrite leaves stale lie inside its span, and no later forward
//! consults them (the note after `forward_move_sources`).
use super::*;
use crate::bytecode::{FieldLayout, ImmediateLayout, InstructionField};
use crate::compile::CompileScratch;

/// Lets the readers of each `Move`'s destination read its source instead, where the move
/// feeds only them (the module doc), and whether any did.
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
        let uses = |instruction: Instr| liveness::uses(instruction.as_wide(), methods, field_sites, superinstructions);
        let definitions = |instruction: Instr| liveness::definitions(instruction.as_wide(), superinstructions);
        let Some(forwarding) = scan_move(function, &analysis, &scope, pc, (&uses, &definitions), &mut readers) else {
            continue;
        };
        for &(at, rewritten) in &readers {
            function.code[at] = rewritten;
            analysis.transfers[at] = (uses(rewritten), definitions(rewritten));
            changed.push(at);
        }
        // The rewritten readers read the source up to the last of them, which the analysis
        // predates: its live sets there gain the source, so no later move reads a stale set.
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

// Why the live sets a forward leaves stale are never read again: forwarding `d = Move s` at
// `pc` into its readers changes liveness only at the positions after `pc` up to the end of
// its span, and only in the bits of `d` and `s`. Before `pc` nothing changes: `s` was live
// there (the move read it) and stays live (a reader reads it, and nothing in the span writes
// it), and `d` was dead there (the move wrote it) and stays dead (no reader reads it any
// more, and the moved value is dead where the span ends). After the span nothing changes, as
// its code is untouched and the span is entered only from the move: a jump target inside it
// would be a protected position, where `move_readers` ends the span. A later forward reads
// live sets only about its own destination `d'`, at positions after its own move: a move
// inside an earlier span writes neither `d` nor `s` (any write of either ends that span
// there, and the writing instruction's own queries lie past it), so `d'` is neither, and the
// stale bits are never the ones asked about.

/// What copy propagation may not cross or rewrite in a function.
struct Scope {
    protected: crate::bitset::BitSet,
    /// Promoted local storage, and the parameters' part of it.
    storage: RegisterSet,
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
            storage: mask(false),
            parameters: mask(true),
        }
    }

    /// Whether the instruction is a plain `Move` of one register to another temporary.
    fn forwards(&self, mov: Instr) -> bool {
        mov.op() == Op::Move && !mov.is_wide()
            && register_bit(mov.result_register()) & self.storage == 0 && mov.register_b() != mov.result_register()
    }
}

impl CodeLiveness {
    /// The registers the handlers covering `pc` read on entry: their targets' live sets.
    ///
    /// A rewrite may change which of two registers holds a value inside a span when no
    /// handler can observe either: an abrupt completion inside the span enters these
    /// handlers with every register as the span left it. Spans end before every protected
    /// position, which includes each handler's start and end, so every instruction of a span
    /// is covered by the handlers covering its first one, and this set answers for all.
    /// Handler targets are protected too, so no rewrite leaves their live sets stale.
    pub(super) fn handler_live(&self, pc: usize) -> RegisterSet {
        self.successors
            .at(pc)
            .filter(|&(_, edge)| edge == crate::bytecode::ControlEdge::Handler)
            .fold(RegisterSet::default(), |live, (target, _)| live | self.live.get(target).copied().unwrap_or_default())
    }
}

type Masks<'m> = (&'m dyn Fn(Instr) -> RegisterSet, &'m dyn Fn(Instr) -> RegisterSet);

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
    if !scope.forwards(mov) || analysis.handler_live(pc) & register_bit(mov.result_register()) != 0 {
        return None;
    }
    let (source, destination) = (mov.register_b(), mov.result_register());
    let (source_bit, destination_bit) = (register_bit(source), register_bit(destination));
    // Legacy `f.arguments` reflection may rewrite a parameter during the instructions between, so
    // a parameter's source forwards to its first live reader only.
    let parameter_source = source_bit & scope.parameters != 0;
    let dead_before = |at: usize| analysis.live[at] & destination_bit == 0;
    let dead_after = |at: usize| liveness::live_out(&analysis.successors, &analysis.live, at) & destination_bit == 0;
    readers.clear();
    let mut live_seen = 0;
    let mut dead = false;
    for at in pc + 1..function.code.len() {
        if scope.protected.contains(at) || scope.in_handler(at) {
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
        let (read, written) = (uses(instruction), definitions(instruction));
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

/// The instructions after the `Move` at `pc` that read its destination while it holds the
/// moved value. The span they lie in starts at the move and runs through instructions that
/// fall through without writing the move's source or destination; it ends before a protected
/// position or the window's end, or at an instruction that writes either register or leaves
/// the block. The moved value must be dead where the span ends: not live
/// into the position it stops before, or not live out of the instruction it stops at unless
/// that instruction overwrites the destination.
fn move_readers(function: &BcFunction, analysis: &CodeLiveness, scope: &Scope, pc: usize) -> Option<Readers> {
    const WINDOW: usize = 8;
    let mov = function.code[pc];
    let (source, destination) = (register_bit(mov.register_b()), register_bit(mov.result_register()));
    let found = |readers: Readers, dead: bool| (dead && !readers.is_empty()).then_some(readers);
    let mut readers = Readers::new();
    let end = (pc + 1 + WINDOW).min(function.code.len());
    for at in pc + 1..end {
        if scope.protected.contains(at) {
            return found(readers, analysis.live[at] & destination == 0);
        }
        let (read, written) = analysis.transfers[at];
        if read & destination != 0 {
            readers.push(at);
        }
        let instruction = fixed_width(function.code[at], &function.wide);
        if written & (source | destination) != 0
            || !instruction.op().control_flow_layout().only_falls_through()
            || instruction.returns_from_frame()
        {
            let dead = written & destination != 0
                || liveness::live_out(&analysis.successors, &analysis.live, at) & destination == 0;
            return found(readers, dead);
        }
    }
    found(readers, analysis.live.get(end).is_none_or(|&live| live & destination == 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `code` after one forwarding pass over a script body holding it.
    fn forward_fixture(code: Vec<Instr>, registers: u16) -> Vec<Instr> {
        let mut program = Engine::specialize_unspecialized("", "rewrite-copies.js").unwrap();
        let function = &mut program.functions[0];
        function.code = code;
        function.registers = registers;
        function.local_registers = Default::default();
        forward_move_sources(function, &[] as &[MethodSiteSpec], &[], &[], &mut None);
        std::mem::take(&mut function.code)
    }

    fn add(result: Register, left: Register, right: Register) -> Instr {
        let (left, right) = (Operand::register(left).0, Operand::register(right).0);
        Instr::new(Op::Binary, result, left, right, BinaryOperator::Addition as u32)
    }

    fn operands(instruction: Instr) -> (Option<Register>, Option<Register>) {
        (instruction.operand_b().register_index(), instruction.operand_c().register_index())
    }

    #[test]
    fn forwards_both_operand_copies_into_one_reader() {
        let code = vec![
            Instr::new(Op::LoadConst, 1, 0, 0, 0),
            Instr::new(Op::LoadConst, 2, 0, 0, 0),
            Instr::new(Op::Move, 3, 1, 0, 0),
            Instr::new(Op::Move, 4, 2, 0, 0),
            add(5, 3, 4),
            Instr::new(Op::Return, 5, 0, 0, 0),
        ];
        let forwarded = forward_fixture(code, 6);
        assert_eq!((forwarded[2].op(), forwarded[3].op()), (Op::Nop, Op::Nop));
        assert_eq!(operands(forwarded[4]), (Some(1), Some(2)));
    }

    #[test]
    fn forwards_a_copy_into_every_reader_of_its_span() {
        let code = vec![
            Instr::new(Op::LoadConst, 1, 0, 0, 0),
            Instr::new(Op::Move, 3, 1, 0, 0),
            add(4, 3, 1),
            add(5, 4, 3),
            Instr::new(Op::Return, 5, 0, 0, 0),
        ];
        let forwarded = forward_fixture(code, 6);
        assert_eq!(forwarded[1].op(), Op::Nop);
        assert_eq!(operands(forwarded[2]), (Some(1), Some(1)));
        assert_eq!(operands(forwarded[3]), (Some(4), Some(1)));
    }

    #[test]
    fn keeps_a_copy_whose_source_changes_before_a_later_reader() {
        let code = vec![
            Instr::new(Op::LoadConst, 1, 0, 0, 0),
            Instr::new(Op::Move, 3, 1, 0, 0),
            add(4, 3, 3),
            Instr::new(Op::LoadConst, 1, 0, 0, 0),
            add(5, 4, 3),
            add(5, 5, 1),
            Instr::new(Op::Return, 5, 0, 0, 0),
        ];
        let forwarded = forward_fixture(code, 6);
        assert_eq!(forwarded[1].op(), Op::Move);
        assert_eq!(operands(forwarded[2]), (Some(3), Some(3)));
    }
}
