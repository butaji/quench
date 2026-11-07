//! Copy propagation over the bytecode: a read of a `Move`'s destination takes its source.
use super::*;

/// Lets the first reader of a `Move`'s destination read the move's source instead when the move
/// feeds only that reader: `d = Move s; …; x(d)` becomes `Nop; …; x(s)`, the forward
/// counterpart of `coalesce_distant_producer_moves`. Control reaches the reader only through the
/// move, by instructions that fall through without writing `s` or `d` (`move_reader`); `d` is
/// dead after it and not promoted local storage (whose writes legacy `f.arguments` reflection
/// reads), and no instruction is inside a handler region. Only register, operand and field-base
/// fields are rewritten; the reader's use and definition masks confirm the rewrite trades `d`
/// for `s` and nothing else. A read of a binding a statement copied before evaluating its other
/// operands (`o.v = o.v + 1`) then reads the binding: each tier sees one value, not a copy.
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
    let Some(analysis) = CodeLiveness::of(current, function, methods, field_sites, superinstructions) else {
        return false;
    };
    let scope = Scope::of(function);
    let mut changed = Vec::new();
    let mut rewritten_at = crate::bitset::BitSet::new(function.code.len());
    for pc in 0..function.code.len() {
        if rewritten_at.contains(pc) {
            continue;
        }
        let uses = |instruction: Instr| liveness::uses(instruction.as_wide(), methods, field_sites, superinstructions);
        let definitions = |instruction: Instr| liveness::definitions(instruction.as_wide(), superinstructions);
        if let Some((at, rewritten)) = forwarded(function, &analysis, &scope, pc, (&uses, &definitions)) {
            function.code[pc] = Instr::new(Op::Nop, 0, 0, 0, 0);
            function.code[at] = rewritten;
            [pc, at].into_iter().for_each(|rewritten| { changed.push(rewritten); rewritten_at.insert(rewritten); });
        }
    }
    hand_on(analysis, &changed, function, methods, field_sites, superinstructions, current)
}

/// What copy propagation may not cross or rewrite in a function.
struct Scope {
    protected: crate::bitset::BitSet,
    /// The pcs some handler covers.
    handled: crate::bitset::BitSet,
    /// Promoted local storage, and the parameters' part of it.
    storage: RegisterSet,
    parameters: RegisterSet,
}

impl Scope {
    fn of(function: &BcFunction) -> Self {
        let mask = |parameters_only: bool| function.local_registers.iter()
            .filter(|entry| !parameters_only || entry.local < function.params)
            .fold(0, |mask, entry| mask | register_bit(entry.register));
        Self {
            protected: protected_positions(&function.code, &function.wide, &function.handlers,
                function.parameter_end_pc, &function.binding_sites),
            handled: handled_pcs(function),
            storage: mask(false),
            parameters: mask(true),
        }
    }

    /// Whether the instruction at `pc` is a plain `Move` of one register to another temporary.
    fn forwards(&self, mov: Instr, pc: usize) -> bool {
        mov.op() == Op::Move && !mov.is_wide() && !self.in_handler(pc)
            && register_bit(mov.result_register()) & self.storage == 0 && mov.register_b() != mov.result_register()
    }

    fn in_handler(&self, pc: usize) -> bool {
        self.handled.contains(pc)
    }
}

/// The pcs of `function` some handler covers, from one sweep over the handlers.
fn handled_pcs(function: &BcFunction) -> crate::bitset::BitSet {
    let mut handled = crate::bitset::BitSet::new(function.code.len());
    crate::bytecode::Handler::for_each_covering(&function.handlers, 0..function.code.len(), |pc, covering| {
        if !covering.is_empty() {
            handled.insert(pc);
        }
    });
    handled
}

type Masks<'m> = (&'m dyn Fn(Instr) -> RegisterSet, &'m dyn Fn(Instr) -> RegisterSet);

/// The reader of the `Move` at `pc` and that reader rewritten to read the move's source.
fn forwarded(function: &BcFunction, analysis: &CodeLiveness, scope: &Scope, pc: usize, (uses, definitions): Masks<'_>)
    -> Option<(usize, Instr)> {
    let mov = function.code[pc];
    if !scope.forwards(mov, pc) {
        return None;
    }
    let (source, destination) = (mov.register_b(), mov.result_register());
    let at = move_reader(function, &analysis.transfers, scope, pc)?;
    // Legacy `f.arguments` reflection may rewrite a parameter during the instructions between.
    let consumer = function.code[at];
    let crossed = at > pc + 1 && register_bit(source) & scope.parameters != 0;
    if crossed || consumer.is_wide()
        || liveness::live_out(&analysis.successors, &analysis.live, at) & register_bit(destination) != 0
    {
        return None;
    }
    let rewritten = substitute_register_read(consumer, destination, source)?;
    let (read, written) = analysis.transfers[at];
    let expected = (read & !register_bit(destination)) | register_bit(source);
    (uses(rewritten) == expected && definitions(rewritten) == written).then_some((at, rewritten))
}

/// The first instruction after the `Move` at `pc` that reads its destination, when control
/// reaches it only from the `Move`, through instructions that fall through without writing its
/// source or destination.
fn move_reader(function: &BcFunction, transfers: &[(RegisterSet, RegisterSet)], scope: &Scope, pc: usize) -> Option<usize> {
    const WINDOW: usize = 8;
    let mov = function.code[pc];
    let (source, destination) = (register_bit(mov.register_b()), register_bit(mov.result_register()));
    for at in (pc + 1..function.code.len()).take(WINDOW) {
        let (read, written) = transfers[at];
        if scope.protected.contains(at) || scope.in_handler(at) {
            return None;
        }
        if read & destination != 0 {
            return Some(at);
        }
        let flow = fixed_width(function.code[at], &function.wide).op().control_flow_layout();
        if written & (source | destination) != 0
            || !flow.only_falls_through()
        {
            return None;
        }
    }
    None
}
