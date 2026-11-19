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

/// Lets the readers of each `Move`'s destination read its source instead, where the move
/// feeds only them (the module doc), and whether any did.
pub(super) fn forward_move_sources(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
    current: &mut Option<CodeFacts>,
) -> bool {
    if !function.code.iter().any(|instruction| instruction.op() == Op::Move) {
        return false;
    }
    let Some(mut analysis) = CodeLiveness::of(current, function, methods, field_sites, superinstructions) else {
        return false;
    };
    let scope = Scope::of(function);
    let mut changed = Vec::new();
    for pc in 0..function.code.len() {
        let uses = |instruction: Instr| liveness::uses(instruction.as_wide(), methods, field_sites, superinstructions);
        let definitions = |instruction: Instr| liveness::definitions(instruction.as_wide(), superinstructions);
        let Some(rewrites) = forwarded(function, &analysis, &scope, pc, (&uses, &definitions)) else {
            continue;
        };
        function.code[pc] = Instr::new(Op::Nop, 0, 0, 0, 0);
        analysis.transfers[pc] = (RegisterSet::default(), RegisterSet::default());
        changed.push(pc);
        for (at, rewritten) in rewrites {
            function.code[at] = rewritten;
            analysis.transfers[at] = (uses(rewritten), definitions(rewritten));
            changed.push(at);
        }
    }
    hand_on(analysis, &changed, function, methods, field_sites, superinstructions, current)
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
        let mask = |parameters_only: bool| function.local_registers.iter()
            .filter(|entry| !parameters_only || entry.local < function.params)
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

/// The readers of a `Move`'s destination, as `move_readers` finds them; most moves have one.
type Readers = smallvec::SmallVec<[usize; 4]>;

/// Each reader of the `Move` at `pc`, rewritten to read the move's source.
fn forwarded(function: &BcFunction, analysis: &CodeLiveness, scope: &Scope, pc: usize, masks: Masks<'_>)
    -> Option<smallvec::SmallVec<[(usize, Instr); 4]>> {
    let mov = function.code[pc];
    if !scope.forwards(mov) || analysis.handler_live(pc) & register_bit(mov.result_register()) != 0 {
        return None;
    }
    let (source, destination) = (mov.register_b(), mov.result_register());
    let readers = move_readers(function, analysis, scope, pc)?;
    // Legacy `f.arguments` reflection may rewrite a parameter during the instructions between.
    if register_bit(source) & scope.parameters != 0 && readers.iter().any(|&at| at > pc + 1) {
        return None;
    }
    readers.into_iter().map(|at| rewritten_reader(function, analysis, at, (source, destination), masks)).collect()
}

/// The reader at `at` reading register `source` where it read `destination`, when its masks
/// confirm the rewrite trades the one register for the other and nothing else.
fn rewritten_reader(
    function: &BcFunction,
    analysis: &CodeLiveness,
    at: usize,
    (source, destination): (Register, Register),
    (uses, definitions): Masks<'_>,
) -> Option<(usize, Instr)> {
    let consumer = function.code[at];
    if consumer.is_wide() {
        return None;
    }
    let rewritten = substitute_register_read(consumer, destination, source)?;
    let (read, written) = analysis.transfers[at];
    let expected = (read & !register_bit(destination)) | register_bit(source);
    (uses(rewritten) == expected && definitions(rewritten) == written).then_some((at, rewritten))
}

/// The instructions after the `Move` at `pc` that read its destination while it holds the
/// moved value. The span they lie in starts at the move and runs through instructions that
/// fall through without writing the move's source or destination; it ends before a protected
/// position or the window's end, or at an instruction that writes either
/// register or leaves the block. The moved value must be dead where the span ends: not live
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
