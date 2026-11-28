use super::*;
use crate::bytecode::{
    ControlFlowLayout, FieldLayout, ImmediateRole, InstructionField, Superinstruction, Traits, WideInstruction,
    negated_jump, op_table, row_ops, strict_variant, the_op, widen, OPCODE_COUNT,
};
use super::liveness::{RegisterSet, register_bit};
mod copies;
use copies::forward_move_sources;
mod loop_constants;
pub(super) use loop_constants::hoist_loop_constants;
pub(super) use crate::bytecode::OpSet;

/// The one op each role names, derived from the catalog. The rewriter spells its ops by role,
/// never by name: `NOP` is the filler of removed instructions, `MOVE` the register copy.
pub(super) const NOP: Op = the_op(Traits::NO_OP);
const MOVE: Op = the_op(Traits::REGISTER_COPY);
const RETURN: Op = the_op(Traits::RETURN_VALUE);
const JUMP: Op = the_op(Traits::UNCONDITIONAL_JUMP);
const JUMP_BINARY_FALSE: Op = the_op(Traits::FUSED_BINARY_JUMP);
const SUPER_CONST_ARRAY_OBJECT2: Op = the_op(Traits::FUSED_CONST_ARRAY_OBJECT2);
/// The `[MakeConstArray, Binary, Binary, MakeObject2]` window `SUPER_CONST_ARRAY_OBJECT2` fuses,
/// in order, each element named by its trait.
pub(super) const CONST_ARRAY_OBJECT2: [Op; 4] = [
    the_op(Traits::CONST_ARRAY_OBJECT2_HEAD),
    the_op(Traits::CONST_ARRAY_OBJECT2_OPERAND),
    the_op(Traits::CONST_ARRAY_OBJECT2_OPERAND),
    the_op(Traits::CONST_ARRAY_OBJECT2_TAIL),
];
/// Each jump's opposite-polarity twin (`JumpTrue` and `JumpFalse`), and each sloppy property store's
/// strict form, both derived from the rows' traits and encodings.
const NEGATED_JUMPS: [Option<Op>; OPCODE_COUNT] = op_table!(None, op => negated_jump(op));
const STRICT_VARIANTS: [Option<Op>; OPCODE_COUNT] = op_table!(None, op => strict_variant(op));

macro_rules! fusion_recipes {
    ($($recipe:ident => $handler:expr;)+) => {
        #[derive(Clone, Copy)]
        enum Recipe { $($recipe),+ }

        fn apply_recipe(
            recipe: Recipe,
            first: Instr,
            second: Instr,
            fields: &mut SiteTable<'_, FieldSite>,
        ) -> Option<Instr> {
            match recipe {
                $(Recipe::$recipe => ($handler)(first, second, fields)),+
            }
        }
    };
}

// Ordered recipe entries are the complete overlap and semantic policy. The
// macro derives recipe identity, pattern rows, and recipe dispatch together.
// An empty pattern list means the recipe's pairs are derived from the opcode rows
// (`rules_for`), never listed by hand.
fusion_recipes! {
    // Derived: a `CONSTANT_PRODUCERS` op, then an op whose B and C are `Operand` slots
    // (`CONSTANT_LEFT_SITES`). Left is tried before right, as the listed pairs were.
    ConstantLeft =>
        |first: Instr, mut second: Instr, _: &mut SiteTable<'_, FieldSite>| {
            let result = first.result_register();
            let constant = Operand::constant(first.constant_index() as u32)?;
            if second.operand_b().register_index() != Some(result) { return None; }
            second.set_operand_b(constant)?;
            if second.operand_c().register_index() == Some(result) {
                second.set_operand_c(constant)?;
            }
            Some(second)
        };
    // Derived: a `CONSTANT_PRODUCERS` op, then any op whose C is an `Operand` (`CONSTANT_RIGHT_SITES`).
    ConstantRight =>
        |first: Instr, mut second: Instr, _: &mut SiteTable<'_, FieldSite>| {
            if second.operand_c().register_index() != Some(first.result_register()) { return None; }
            second.set_operand_c(Operand::constant(first.constant_index() as u32)?)?;
            Some(second)
        };
    // Derived: a `FRAME_LOCAL_STORE` op, then a `FRAME_LOCAL_LOAD` op of the same slot. The slot
    // check stays here, a per-instruction condition.
    StoreLoadLocal =>
        |mut first: Instr, second: Instr, _: &mut SiteTable<'_, FieldSite>| {
            if first.optional_register_b().is_some() || first.local_slot() != second.local_slot() { return None; }
            first.set_optional_register_b(Some(second.result_register()));
            Some(first)
        };
    // Derived: a `LOCAL_READERS` op, then a `FIELD_LOOKUPS` op reading its result as the base.
    LocalField =>
        |first: Instr, mut second: Instr, _: &mut SiteTable<'_, FieldSite>| {
            if first.numeric_local_store_target().is_some() || second.writes_current_this() {
                return None;
            }
            let crate::bytecode::FieldLookup::Atom { base, .. } = second.field_lookup()? else {
                return None;
            };
            if base.register_index() != Some(first.result_register()) {
                return None;
            }
            let local = u16::try_from(first.local_slot()).ok()?;
            // A local past the packed field domain keeps the pair.
            second.try_set_b(crate::bytecode::FieldBase::local(local)?.0)?;
            Some(second)
        };
    // Derived like `ReturnResult`: every op whose row writes one plain result (`plain_producer`).
    ProducerMove => |mut first: Instr, second: Instr, _: &mut SiteTable<'_, FieldSite>| {
        if second.register_b() != first.result_register() { return None; }
        first.set_result_register(second.result_register());
        Some(first)
    };
    // Derived: a `BINARY_RESULT` op, then a `JUMPS_ON_FALSE` op. The comparison-only condition is a
    // property of the operator (`fuses_with_jump_operator`), a per-instruction check kept here.
    BinaryJumpFalse =>
        |first: Instr, second: Instr, _: &mut SiteTable<'_, FieldSite>| {
            if first.result_register() != second.register_a() { return None; }
            // Only a comparison fuses (`ast::fuses_with_jump`): an arithmetic or bitwise
            // result stays a Number row whose truthiness the jump tests.
            if !super::ast::fuses_with_jump_operator(first.binary_operator()) { return None; }
            Some(Instr::new(
                JUMP_BINARY_FALSE,
                first.binary_operator() as u16,
                first.operand_b().0,
                first.operand_c().0,
                second.jump_target(),
            ))
        };
    // Its pairs are derived, not listed: every op whose row declares a returnable result
    // (`ResultLayout::allows_return`) followed by the `Return` of that result (`rules_for`).
    ReturnResult => |mut first: Instr, second: Instr, _: &mut SiteTable<'_, FieldSite>| {
        if first.result_register() != second.register_a() { return None; }
        first.set_returns_from_frame();
        Some(first)
    };
    // Derived: a `ReturnableAndThis` op (`allows_this_write`, which is `GetField` alone), then a
    // `THIS_FIELD_SINK` op. `SetThisFieldStrict` is untagged, so a strict store stays a separate op.
    GetSetThis =>
        |mut first: Instr, second: Instr, fields: &mut SiteTable<'_, FieldSite>| {
            if first.result_register() != second.register_a() { return None; }
            let sink = (second.atom_index(), second.cache_site_index());
            match first.field_lookup()? {
                // A nested read sinks only where the lane's sinking row answers it; measured on richards, most
                // sinks took the general read and store (`row_get_field_sink`), so the pair stays two rows.
                crate::bytecode::FieldLookup::Site(_) if !SINK_NESTED_READS => return None,
                crate::bytecode::FieldLookup::Site(index) => {
                    let site = fields.get_mut(index)?;
                    if site.sink.is_some() { return None; }
                    site.sink = Some(sink);
                }
                // A one-hop read keeps its own row and the store its own: the lane's sinking row plans
                // only a nested read's two hops (`NESTED_SITE`), so a one-hop sink took the general
                // read and store every time.
                crate::bytecode::FieldLookup::Atom { .. } => return None,
            }
            first.set_this_result();
            Some(first)
        };
}

/// Whether `GetSetThis` fuses a nested read (`FieldLookup::Site`) with its store.
const SINK_NESTED_READS: bool = false;

pub(super) fn apply(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &mut SiteTable<'_, FieldSite>,
    superinstructions: &mut SiteTable<'_, Superinstruction>,
    scratch: &mut CompileScratch,
) {
    let input = function.code.len();
    thread_jumps(function);
    invert_branches_over_jumps(function);
    let mut changed_passes = 0;
    while rewrite_super_window(function, superinstructions) {
        changed_passes += 1;
    }
    // Each pass hands the liveness of the code it leaves unchanged to the next.
    let mut liveness = None;
    if coalesce_distant_producer_moves(function, methods, field_sites.as_slice(), superinstructions.as_slice(), &mut liveness, scratch) {
        changed_passes += 1;
    }
    if forward_move_sources(function, methods, field_sites.as_slice(), superinstructions.as_slice(), &mut liveness, scratch) {
        changed_passes += 1;
    }
    if rewrite_once(function, methods, field_sites, superinstructions.as_slice(), &mut liveness, scratch) {
        changed_passes += 1;
    }
    if let Some(facts) = liveness {
        facts.recycle(scratch);
    }
    if crate::trace::Channel::Rewrite.on() {
        eprintln!(
            "{{\"kind\":\"quench-rewrite\",\"input\":{input},\"output\":{},\"changed_passes\":{changed_passes},\"scans\":{}}}",
            function.code.len(),
            changed_passes + 1
        );
    }
}

/// Coalesces the copies frame-register promotion leaves: each promoted load or store of a
/// local becomes a `Move` beside the instruction that produces or consumes its value. The
/// same producer and consumer coalescing as `apply` folds them away, one round of each, as
/// `apply` makes its own: later rounds' code saved less run time than their compile cost.
pub(super) fn coalesce_promoted_moves(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
    scratch: &mut CompileScratch,
) {
    // The `Nop`s the passes leave in place are dropped by the one compaction `lower_settled` ends with.
    let mut liveness = None;
    coalesce_distant_producer_moves(function, methods, field_sites, superinstructions, &mut liveness, scratch);
    forward_move_sources(function, methods, field_sites, superinstructions, &mut liveness, scratch);
    remove_dead_moves(function, methods, field_sites, superinstructions, &mut liveness, scratch);
    if let Some(facts) = liveness {
        facts.recycle(scratch);
    }
}

/// Removes each `Move` or `LoadConst` whose destination no path reads before writing it again:
/// the copy into a temporary or a promoted local that coalescing left behind once its readers
/// moved to the source, or the `undefined` a promoted local was opened with before its first
/// real write. Both are pure and write only their result, so a dead one is unobservable. A
/// parameter's register is kept, as legacy `f.arguments` reflection reads it.
fn remove_dead_moves(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
    current: &mut Option<CodeFacts>,
    scratch: &mut CompileScratch,
) -> bool {
    let removable = |i: Instr| i.op().traits().has(Traits::REMOVABLE_WHEN_DEAD) && !i.is_wide() && i.result_flags() == 0;
    if !function.code.iter().any(|&instruction| removable(instruction)) {
        return false;
    }
    let Some(analysis) = CodeLiveness::of(current, function, methods, field_sites, superinstructions, scratch)
    else {
        return false;
    };
    let protected = function_protected_positions(function);
    let parameter_storage = function
        .local_registers
        .iter()
        .filter(|entry| entry.local < function.params)
        .fold(RegisterSet::default(), |mask, entry| mask | register_bit(entry.register));
    let mut changed = Vec::new();
    for (pc, is_protected) in (0..function.code.len()).map(|pc| (pc, protected.contains(pc))) {
        let mov = function.code[pc];
        if !removable(mov) || is_protected {
            continue;
        }
        let destination = register_bit(mov.result_register());
        if destination & parameter_storage != 0
            || liveness::live_out(&analysis.successors, &analysis.live, pc) & destination != 0
        {
            continue;
        }
        function.code[pc] = Instr::new(NOP, 0, 0, 0, 0);
        changed.push(pc);
    }
    hand_on(analysis, &changed, function, methods, field_sites, superinstructions, current)
}

/// Hands `analysis` on to the next pass after the instructions at `changed` were rewritten in
/// place, and whether any was.
fn hand_on(
    analysis: CodeLiveness,
    changed: &[usize],
    function: &BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
    current: &mut Option<CodeFacts>,
) -> bool {
    *current = Some(if changed.is_empty() {
        CodeFacts::Live(analysis)
    } else {
        analysis.rewritten(changed, function, methods, field_sites, superinstructions)
    });
    !changed.is_empty()
}

/// Drops every `Nop`, relocating jumps, handlers and source positions onto the instruction
/// that followed it. The per-pc live sets, when given, are compacted with the same map: a `Nop`
/// passes its successor's live set through, so a dropped entry loses nothing.
pub(super) fn remove_nops(function: &mut BcFunction, live: &mut Option<Vec<RegisterSet>>, scratch: &mut CompileScratch) {
    if !function.code.iter().any(|instruction| instruction.op() == NOP) {
        return;
    }
    let old = std::mem::take(&mut function.code);
    let mut code = scratch.code_buffer(old.len());
    let map = scratch.remap(old.len());
    for (pc, &instruction) in old.iter().enumerate() {
        map[pc] = code.len();
        if instruction.op() != NOP {
            if let Some(live) = live.as_mut() {
                live[code.len()] = live[pc];
            }
            code.push(instruction);
        }
    }
    map[old.len()] = code.len();
    if let Some(live) = live.as_mut() {
        live[code.len()] = live[old.len()];
        live.truncate(code.len() + 1);
    }
    relocate(
        &mut code,
        &mut function.wide,
        &*map,
        &mut function.handlers,
        &mut function.parameter_end_pc,
        &mut function.binding_sites,
        &mut function.source_positions,
        &old,
    );
    function.code = code;
    scratch.recycle_code(old);
}

/// The liveness of a function's code as it stands: each instruction's transfer masks and the
/// registers live before it.
struct CodeLiveness {
    transfers: Vec<(RegisterSet, RegisterSet)>,
    live: Vec<RegisterSet>,
    /// The code's successor table, which in-place `Move` and producer rewrites leave intact.
    successors: crate::bytecode::Successors,
}

/// What a pass hands the next about the code as it stands. A pass that leaves the code
/// unchanged hands on its liveness; one that rewrites instructions in place, or only drops
/// `Nop`s, hands on the transfer masks, which depend on each instruction alone and are
/// updated for the instructions it changed, so the next pass derives only the live sets.
enum CodeFacts {
    Live(CodeLiveness),
    Transfers(Vec<(RegisterSet, RegisterSet)>),
}

impl CodeFacts {
    /// Recycles the tables of the facts a pass chain ends with into `scratch`.
    fn recycle(self, scratch: &mut CompileScratch) {
        match self {
            CodeFacts::Live(analysis) => analysis.recycle(scratch),
            CodeFacts::Transfers(transfers) => scratch.recycle_transfers(transfers),
        }
    }
}

impl CodeLiveness {
    /// The liveness `current` hands on or completes, otherwise derived from the code, with its
    /// transfer and successor tables taken from `scratch`.
    fn of(
        current: &mut Option<CodeFacts>,
        function: &BcFunction,
        methods: &(impl liveness::MethodArguments + ?Sized),
        field_sites: &[FieldSite],
        superinstructions: &[Superinstruction],
        scratch: &mut CompileScratch,
    ) -> Option<Self> {
        let transfers = match current.take() {
            Some(CodeFacts::Live(analysis)) => return Some(analysis),
            Some(CodeFacts::Transfers(transfers)) => {
                debug_assert!(
                    liveness::transfers(function, methods, field_sites, superinstructions)
                        .is_some_and(|derived| derived == transfers),
                    "handed-on transfer masks match the code"
                );
                transfers
            }
            None => {
                let mut transfers = std::mem::take(&mut scratch.transfers);
                if !liveness::buffers::transfers_into(function, methods, field_sites, superinstructions, &mut transfers) {
                    scratch.recycle_transfers(transfers);
                    return None;
                }
                transfers
            }
        };
        let mut successors = std::mem::take(&mut scratch.successors);
        successors.refill(function);
        let live = liveness::analyze_wide_with(function, &transfers, &successors, scratch);
        Some(Self { transfers, live, successors })
    }

    /// Keeps this analysis's tables as the buffers of the next one.
    fn recycle(self, scratch: &mut CompileScratch) {
        scratch.recycle_transfers(self.transfers);
        scratch.recycle_successors(self.successors);
    }

    /// Updates the facts for one span `coalesce_distant_producer_moves` coalesced. A span is a
    /// block (one entry, falling through, under handlers that read neither register) where
    /// nothing between the producer and the move touches either register, so only the live
    /// sets after the producer change:
    /// `src` is no longer read there and `dst` holds the value instead, live up to the move
    /// exactly when it is live after it. The producer's own live set is unchanged.
    fn coalesce(
        &mut self,
        span: &CoalescedSpan,
        function: &BcFunction,
        methods: &(impl liveness::MethodArguments + ?Sized),
        field_sites: &[FieldSite],
        superinstructions: &[Superinstruction],
    ) {
        for live in &mut self.live[span.producer + 1..=span.target] {
            *live = (*live & !span.source) | span.destination_out;
        }
        update_transfers(
            &mut self.transfers,
            &[span.producer, span.target],
            function,
            methods,
            field_sites,
            superinstructions,
        );
    }

    /// The facts to hand on after the instructions at `changed` were rewritten in place.
    fn rewritten(
        self,
        changed: &[usize],
        function: &BcFunction,
        methods: &(impl liveness::MethodArguments + ?Sized),
        field_sites: &[FieldSite],
        superinstructions: &[Superinstruction],
    ) -> CodeFacts {
        let mut transfers = self.transfers;
        update_transfers(&mut transfers, changed, function, methods, field_sites, superinstructions);
        CodeFacts::Transfers(transfers)
    }
}

/// Re-derives the transfer masks of the instructions at `changed`, which were rewritten in place.
fn update_transfers(
    transfers: &mut [(RegisterSet, RegisterSet)],
    changed: &[usize],
    function: &BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
) {
    for &pc in changed {
        let instruction = function.widened(function.code[pc]);
        transfers[pc] = liveness::transfer(instruction, methods, field_sites, superinstructions);
    }
}

/// Retargets each unconditional jump whose destination is another unconditional jump to the
/// end of that chain. Only `Jump` sources are threaded: it is the back-edge collection safepoint,
/// so every retargeted back edge still collects.
fn thread_jumps(function: &mut BcFunction) {
    let length = function.code.len();
    for pc in 0..length {
        let mut instruction = function.code[pc];
        if instruction.op() != JUMP || instruction.is_wide() {
            continue;
        }
        let mut target = instruction.jump_target();
        // A chain longer than the function is a cycle of jumps; any member is equivalent.
        for _ in 0..length {
            match function.code.get(target as usize) {
                Some(next)
                    if next.op() == JUMP && !next.is_wide() && next.jump_target() != target =>
                {
                    target = next.jump_target();
                }
                _ => break,
            }
        }
        if target != instruction.jump_target() && instruction.try_set_jump_target(target) {
            function.code[pc] = instruction;
        }
    }
}

/// Folds a truthiness branch over a forward unconditional jump into one inverted branch:
/// `JumpTrue a, L; Jump M; L:` becomes `JumpFalse a, M` (and symmetrically). The skipped `Jump`
/// becomes a `Nop` that compaction removes; it must be unreachable except by fallthrough, and
/// must not be a back edge, which is the collection safepoint that `thread_jumps` preserves.
fn invert_branches_over_jumps(function: &mut BcFunction) {
    let protected = protected_positions(
        &function.code,
        &function.wide,
        &function.handlers,
        function.parameter_end_pc,
        &function.binding_sites,
    );
    for pc in 0..function.code.len().saturating_sub(1) {
        let (branch, skipped) = (function.code[pc], function.code[pc + 1]);
        if branch.is_wide() || skipped.is_wide() {
            continue;
        }
        let Some(inverted) = NEGATED_JUMPS[branch.op() as usize] else {
            continue;
        };
        if protected.contains(pc + 1)
            || skipped.op() != JUMP
            || branch.jump_target() as usize != pc + 2
            || skipped.jump_target() as usize <= pc + 1
        {
            continue;
        }
        let target = skipped.jump_target();
        function.code[pc] = Instr::new(inverted, branch.register_a(), 0, 0, target);
        function.code[pc + 1] = Instr::new(NOP, 0, 0, 0, 0);
    }
}

/// Retargets a producer to the destination of a later `Move` of its result, the
/// non-adjacent form of `ProducerMove`: `src = producer; ...; dst = Move src` becomes
/// `dst = producer; ...; Nop`. Call arguments are evaluated before their window is
/// reserved, so each argument computed ahead of another argument's evaluation ends in such
/// a move. Writing `dst` earlier is unobservable when the span between the two is one basic
/// block that neither reads nor writes `dst` or `src`, the handlers covering the producer read
/// neither register (`CodeLiveness::handler_live`; the span lies under one set of handlers,
/// since its boundaries are protected positions), `src` is dead after the move, and neither
/// register is promoted local storage (which legacy `f.arguments` reflection reads). Each
/// coalesced span updates the transfer masks and live sets it changes before the next move is
/// read (`CodeLiveness::coalesce`), so spans may nest and chain within one pass.
fn coalesce_distant_producer_moves(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
    current: &mut Option<CodeFacts>,
    scratch: &mut CompileScratch,
) -> bool {
    // Liveness is the expensive part; a function without a `Move` has nothing to coalesce.
    if !function
        .code
        .iter()
        .any(|instruction| instruction.op() == MOVE)
    {
        return false;
    }
    let Some(mut analysis) = CodeLiveness::of(current, function, methods, field_sites, superinstructions, scratch)
    else {
        return false;
    };
    let protected = protected_positions(
        &function.code,
        &function.wide,
        &function.handlers,
        function.parameter_end_pc,
        &function.binding_sites,
    );
    let storage = function
        .local_registers
        .iter()
        .fold(RegisterSet::default(), |mask, entry| mask | register_bit(entry.register));
    let mut changed = Vec::new();
    // A move of a register onto itself may have kept the register live upstream, so after
    // one is dropped the live sets are derived anew and no span is coalesced on stale ones.
    let mut live_exact = true;
    for target in 0..function.code.len() {
        let mov = function.code[target];
        if mov.op() != MOVE || mov.is_wide() || protected.contains(target) {
            continue;
        }
        let (source, destination) = (mov.register_b(), mov.result_register());
        let pair = register_bit(source) | register_bit(destination);
        if source == destination {
            function.code[target] = Instr::new(NOP, 0, 0, 0, 0);
            changed.push(target);
            live_exact = false;
            continue;
        }
        if !live_exact
            || pair.count_ones() != 2
            || pair & storage != 0
            || liveness::live_out(&analysis.successors, &analysis.live, target) & register_bit(source)
                != 0
        {
            continue;
        }
        let Some(producer) = block_producer(function, &protected, &analysis.transfers, target, pair) else {
            continue;
        };
        let instruction = function.code[producer];
        if instruction.is_wide()
            || analysis.handler_live(producer) & pair != 0
            || !coalesces_at_a_distance(instruction.op())
            || instruction.result_register() != source
            || analysis.transfers[producer].1 != register_bit(source)
            || analysis.transfers[producer].0 & pair != 0
        {
            continue;
        }
        let mut retargeted = instruction;
        retargeted.set_result_register(destination);
        function.code[producer] = retargeted;
        function.code[target] = Instr::new(NOP, 0, 0, 0, 0);
        changed.extend([producer, target]);
        let span = CoalescedSpan {
            producer,
            target,
            source: register_bit(source),
            destination_out: register_bit(destination) & analysis.live[target + 1],
        };
        analysis.coalesce(&span, function, methods, field_sites, superinstructions);
    }
    if live_exact {
        debug_assert!(
            liveness::analyze_transfers(function, &analysis.transfers) == analysis.live,
            "coalesced liveness matches the code"
        );
        *current = Some(CodeFacts::Live(analysis));
        return !changed.is_empty();
    }
    hand_on(analysis, &changed, function, methods, field_sites, superinstructions, current)
}

/// The producer a `Move` at `target` of a register in `pair` coalesces with: the nearest
/// instruction before it that reads or writes either register, when everything from it to
/// the move is one basic block (no instruction after it is a jump target or handler boundary,
/// and none from it to the move leaves the block).
fn block_producer(
    function: &BcFunction,
    protected: &crate::bitset::BitSet,
    transfers: &[(RegisterSet, RegisterSet)],
    target: usize,
    pair: RegisterSet,
) -> Option<usize> {
    for pc in (0..target).rev() {
        if protected.contains(pc + 1) {
            return None;
        }
        let instruction = function.widened(function.code[pc]);
        if !instruction.op().control_flow_layout().only_falls_through() || instruction.returns_from_frame()
        {
            return None;
        }
        let (read, written) = transfers[pc];
        if (read | written) & pair != 0 {
            return Some(pc);
        }
    }
    None
}

/// A `src = producer; ...; dst = Move src` that `coalesce_distant_producer_moves` turned into
/// `dst = producer; ...; Nop`, with the bit of `src` and the bit of `dst` if live after the move.
struct CoalescedSpan {
    producer: usize,
    target: usize,
    source: RegisterSet,
    destination_out: RegisterSet,
}

/// `instruction` reading register `to` wherever a register, operand or field-base field reads
/// `from`; `None` when a rewritten field does not fit the packed form.
fn substitute_register_read(instruction: Instr, from: Register, to: Register) -> Option<Instr> {
    let op = instruction.op();
    let fields = InstructionField::ALL.iter().map(|&field| {
        let value = instruction.field_value(field);
        match op.field_layout(field) {
            FieldLayout::Register if value == from => to,
            layout
                if layout.is_operand_field() && Operand(value).register_index() == Some(from) =>
            {
                Operand::register(to).0
            }
            FieldLayout::FieldBase if FieldBase(value).register_index() == Some(from) => to,
            _ => value,
        }
    });
    let [a, b, c] = <[u16; 3]>::try_from(fields.collect::<Vec<_>>()).ok()?;
    Instr::try_new(op, a, b, c, instruction.imm())
}

/// Lets a producer whose fallthrough successor returns its result return directly, including
/// when that `Return` is a jump target, the case `ReturnResult` cannot fuse; the `Return` stays
/// for its other predecessors. A `Return` on a handler or parameter-region boundary is left
/// alone: returning on its other side would cross the region. A binding site's resume PC only
/// projects the preceding call's lexical scope, so it does not block the flag. Run on every
/// function, so each producer that falls into a `Return` of its result carries the flag and
/// dispatch needs no look-ahead for terminal calls.
pub(super) fn return_through_shared_returns(function: &mut BcFunction) {
    // Only a return's producer is decoded, and the region boundaries are found once one is.
    let mut boundaries = None;
    for pc in 1..function.code.len() {
        if function.instruction_at(pc).op() != RETURN {
            continue;
        }
        // The producer is the nearest instruction before the `Return` that is not a `Nop`.
        let Some(producer_pc) = (0..pc).rev().find(|&at| function.code[at].op() != NOP) else {
            continue;
        };
        let (producer, next) = (function.instruction_at(producer_pc), function.instruction_at(pc));
        if !fuses(producer.op(), RETURN, |recipe| matches!(recipe, Recipe::ReturnResult))
            || producer.result_flags() != 0
            || producer.result_register() != next.register_a()
        {
            continue;
        }
        let boundaries = boundaries
            .get_or_insert_with(|| region_boundaries(function.code.len(), &function.handlers, function.parameter_end_pc, &[]));
        if (producer_pc + 1..=pc).any(|at| boundaries.contains(at)) {
            continue;
        }
        let packed = &mut function.code[producer_pc];
        if packed.is_wide() {
            crate::bytecode::wide_entry_mut(&mut function.wide, *packed).set_returns_from_frame();
        } else {
            packed.set_returns_from_frame();
        }
    }
}

/// Spells a strict function's property sets as their strict ops, so the op alone carries the
/// set's strictness and dispatch need not consult the function.
pub(super) fn express_strict_sets(function: &mut BcFunction) {
    if !function.strict {
        return;
    }
    let strict_op = |op: Op| STRICT_VARIANTS[op as usize];
    for packed in &mut function.code {
        if packed.is_wide() {
            let wide = crate::bytecode::wide_entry_mut(&mut function.wide, *packed);
            if let Some(op) = strict_op(wide.op()) {
                wide.set_op(op);
            }
        } else if let Some(op) = strict_op(packed.op()) {
            packed.set_op(op);
        }
    }
}

/// Whether `op`'s row makes one value into its result register and nothing else of its
/// registers: field A is the result, which it does not read; no other field writes a register
/// or names a window the result might fall in; it falls through or calls. Writing that value to
/// another register is then the same operation, which `ProducerMove` retargets to a `Move`'s
/// destination. A suspension (`Await`, `Yield`) writes its result on resume, from a frame the
/// tiers rebuild, so it keeps its register.
pub(super) const fn plain_producer(op: Op) -> bool {
    const fn other_field_plain(layout: FieldLayout) -> bool {
        !matches!(
            layout,
            FieldLayout::ResultRegister | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister
                | FieldLayout::OptionalRegister | FieldLayout::RegisterWindowBase | FieldLayout::RegisterCount
        )
    }
    matches!(op.field_layout(InstructionField::A), FieldLayout::ResultRegister)
        && other_field_plain(op.field_layout(InstructionField::B))
        && other_field_plain(op.field_layout(InstructionField::C))
        && !op.result_layout().reads_result_register()
        && matches!(op.control_flow_layout(), ControlFlowLayout::Fallthrough | ControlFlowLayout::Call)
        && !op.traits().has(Traits::REGISTER_COPY)
        && !op.traits().has(Traits::SUSPENDS)
}

/// Which ops are `plain_producer`s: the rows that make one value into their result (`ProducerMove`).
const PLAIN_PRODUCERS: OpSet = row_ops!(op => plain_producer(op));

/// Whether `op`'s row makes a constant its result: the immediate is a constant-pool index, read
/// into the result register alone. `MakeConstArray` and `StringCaseIndex` also name an element
/// count, so they are not constant producers.
const fn constant_producer(op: Op) -> bool {
    matches!(op.immediate_role(), ImmediateRole::ConstantIndex)
        && matches!(op.field_layout(InstructionField::A), FieldLayout::ResultRegister)
        && matches!(op.field_layout(InstructionField::B), FieldLayout::Unused)
        && matches!(op.field_layout(InstructionField::C), FieldLayout::Unused)
}

/// Which ops are `constant_producer`s (`ConstantLeft` and `ConstantRight`'s first op): `LoadConst`.
const CONSTANT_PRODUCERS: OpSet = row_ops!(op => constant_producer(op));

/// Ops whose B and C fields are both `Operand` slots. A constant can fill either, and the
/// `ConstantLeft` handler rewrites C as well as B, so it needs both to be operand slots.
const CONSTANT_LEFT_SITES: OpSet = row_ops!(op =>
    matches!(op.field_layout(InstructionField::B), FieldLayout::Operand)
        && matches!(op.field_layout(InstructionField::C), FieldLayout::Operand));

/// Ops whose C field is an `Operand` slot, which `ConstantRight` rewrites.
const CONSTANT_RIGHT_SITES: OpSet = row_ops!(op => matches!(op.field_layout(InstructionField::C), FieldLayout::Operand));

/// Ops whose row reads a local into its result: a `ResultRegister` with a `NumericLocalTarget`
/// in B (`LoadLocal` and `LoadLocalPlain`). `LoadEnvLocal` reads an environment slot and has no
/// such B field.
pub(super) const LOCAL_READERS: OpSet = row_ops!(op =>
    matches!(op.field_layout(InstructionField::A), FieldLayout::ResultRegister)
        && matches!(op.field_layout(InstructionField::B), FieldLayout::NumericLocalTarget));

/// Ops whose B field is a `FieldBase` looked up by a `FieldLookup` immediate: the base of a field
/// read (`GetField`). `LocalField`'s second op.
const FIELD_LOOKUPS: OpSet = row_ops!(op =>
    matches!(op.field_layout(InstructionField::B), FieldLayout::FieldBase)
        && matches!(op.immediate_role(), ImmediateRole::FieldLookup));

/// Ops tagged `FRAME_LOCAL_STORE`: the first op of `StoreLoadLocal`.
const FRAME_LOCAL_STORES: OpSet = row_ops!(op => op.traits().has(Traits::FRAME_LOCAL_STORE));

/// Ops tagged `FRAME_LOCAL_LOAD`: the second op of `StoreLoadLocal`.
const FRAME_LOCAL_LOADS: OpSet = row_ops!(op => op.traits().has(Traits::FRAME_LOCAL_LOAD));

/// Ops tagged `BINARY_RESULT`: the first op of `BinaryJumpFalse`.
const BINARY_RESULTS: OpSet = row_ops!(op => op.traits().has(Traits::BINARY_RESULT));

/// Ops tagged `JUMPS_ON_FALSE`: the second op of `BinaryJumpFalse`.
const JUMPS_ON_FALSE: OpSet = row_ops!(op => op.traits().has(Traits::JUMPS_ON_FALSE));

/// Ops whose result may be written to `this` (`ResultLayout::allows_this_write`): the first op of `GetSetThis`.
const THIS_WRITE_SOURCES: OpSet = row_ops!(op => op.result_layout().allows_this_write());

/// Ops tagged `THIS_FIELD_SINK`: the second op of `GetSetThis`.
const THIS_FIELD_SINKS: OpSet = row_ops!(op => op.traits().has(Traits::THIS_FIELD_SINK));

/// Ops whose result may carry a numeric local target (`ResultLayout::allows_numeric_local`):
/// the arithmetic rows `Binary`, `NumericAdd`, `NumericMultiply`. Their store-compaction rules
/// use it as the first op.
pub(super) const NUMERIC_LOCAL_PRODUCERS: OpSet = row_ops!(op => op.result_layout().allows_numeric_local());

/// Ops whose B and C are both `NumericIndexOperand` slots: the element read `GetIndex`, which
/// takes two local operands as indices.
pub(super) const NUMERIC_INDEX_CONSUMERS: OpSet = row_ops!(op =>
    matches!(op.field_layout(InstructionField::B), FieldLayout::NumericIndexOperand)
        && matches!(op.field_layout(InstructionField::C), FieldLayout::NumericIndexOperand));

/// The recipes a pair's rows imply, in recipe order. Each set above states one row property the
/// recipe relies on, so the recipe holds for every op that has it.
fn rules_for(first: Op, second: Op) -> &'static [Recipe] {
    let (first_index, second_index) = (first as usize, second as usize);
    if second == RETURN {
        // An op whose row declares a returnable result completes its function itself when a
        // `Return` of that result follows it.
        if first.result_layout().allows_return() { &[Recipe::ReturnResult] } else { &[] }
    } else if second == MOVE {
        if PLAIN_PRODUCERS[first_index] { &[Recipe::ProducerMove] } else { &[] }
    } else if CONSTANT_PRODUCERS[first_index] && CONSTANT_LEFT_SITES[second_index] {
        &[Recipe::ConstantLeft, Recipe::ConstantRight]
    } else if CONSTANT_PRODUCERS[first_index] && CONSTANT_RIGHT_SITES[second_index] {
        &[Recipe::ConstantRight]
    } else if LOCAL_READERS[first_index] && FIELD_LOOKUPS[second_index] {
        &[Recipe::LocalField]
    } else if FRAME_LOCAL_STORES[first_index] && FRAME_LOCAL_LOADS[second_index] {
        &[Recipe::StoreLoadLocal]
    } else if BINARY_RESULTS[first_index] && JUMPS_ON_FALSE[second_index] {
        &[Recipe::BinaryJumpFalse]
    } else if THIS_WRITE_SOURCES[first_index] && THIS_FIELD_SINKS[second_index] {
        &[Recipe::GetSetThis]
    } else {
        &[]
    }
}

/// Whether a recipe of the selected kind fuses `[first, second]`.
fn fuses(first: Op, second: Op, recipe: impl Fn(Recipe) -> bool) -> bool {
    rules_for(first, second).iter().any(|&rule| recipe(rule))
}

/// Whether `op` may compute its result straight into a later `Move`'s destination when nothing
/// between them touches either register (`coalesce_distant_producer_moves`): the adjacent
/// `ProducerMove` producers, and the `DISTANT_PRODUCER` rows. Some rows are not `ProducerMove`
/// producers (`MakeObjectLiteral` and `StringConcat` take a register window, which
/// `plain_producer` excludes), and the adjacent recipe keeps its own list, since it also lets a
/// producer read its new destination. The pass refuses a producer that reads either register,
/// so no row here reads the register it writes.
fn coalesces_at_a_distance(op: Op) -> bool {
    fuses(op, MOVE, |recipe| matches!(recipe, Recipe::ProducerMove)) || op.traits().has(Traits::DISTANT_PRODUCER)
}

struct SuperRule {
    pattern: &'static [Op],
    fuse: fn(&[Instr], &mut SiteTable<'_, Superinstruction>) -> Option<Instr>,
}


/// No superinstruction rules: a fused record would carry a second meaning for its parts, so
/// `MakeConstArray`, `Binary` and `MakeObject2` run as their own generated rows.
const SUPER_RULES: &[SuperRule] = &[];

fn rewrite_super_window(
    function: &mut BcFunction,
    superinstructions: &mut SiteTable<'_, Superinstruction>,
) -> bool {
    // The windows are over the live instructions: a `Nop` an earlier fusion left in place is
    // skipped, and the pcs it occupies are covered by the protected check of the window.
    let live = live_positions(&function.code);
    let matches_rule = |rule: &SuperRule| {
        live.windows(rule.pattern.len()).any(|window| {
            window.iter().map(|&pc| function.code[pc].op()).eq(rule.pattern.iter().copied())
        })
    };
    if !SUPER_RULES.iter().any(matches_rule) {
        return false;
    }
    let protected = protected_positions(
        &function.code,
        &function.wide,
        &function.handlers,
        function.parameter_end_pc,
        &function.binding_sites,
    );
    let mut changed = false;
    let mut at = 0;
    while at < live.len() {
        let fused = SUPER_RULES.iter().find_map(|rule| {
            let width = rule.pattern.len();
            let positions = live.get(at..at + width)?;
            let (first, last) = (positions[0], positions[width - 1]);
            if (first + 1..=last).any(|pc| protected.contains(pc))
                || positions.iter().any(|&pc| function.code[pc].is_wide())
                || positions.iter().map(|&pc| function.code[pc].op()).ne(rule.pattern.iter().copied())
            {
                return None;
            }
            let window: Vec<Instr> = positions.iter().map(|&pc| function.code[pc]).collect();
            (rule.fuse)(&window, superinstructions).map(|instruction| (instruction, width))
        });
        match fused {
            Some((instruction, width)) => {
                let positions = &live[at..at + width];
                function.code[positions[0]] = instruction;
                for &pc in &positions[1..] {
                    function.code[pc] = Instr::new(NOP, 0, 0, 0, 0);
                }
                at += width;
                changed = true;
            }
            None => at += 1,
        }
    }
    changed
}

/// The pcs of `code`'s instructions that are not `Nop`s, in order.
pub(super) fn live_positions(code: &[Instr]) -> Vec<usize> {
    (0..code.len()).filter(|&pc| code[pc].op() != NOP).collect()
}

/// The first pc after `pc` whose instruction is not a `Nop`: the instruction control reaches
/// next, since a `Nop` falls through. `None` past the end.
pub(super) fn next_live(code: &[Instr], pc: usize) -> Option<usize> {
    (pc + 1..code.len()).find(|&next| code[next].op() != NOP)
}

/// How many of `code`'s instructions are not `Nop`s: the count the code would have compacted.
pub(super) fn live_count(code: &[Instr]) -> usize {
    code.iter().filter(|instruction| instruction.op() != NOP).count()
}

/// `protected_positions` of `function` as it stands.
fn function_protected_positions(function: &BcFunction) -> crate::bitset::BitSet {
    protected_positions(&function.code, &function.wide, &function.handlers, function.parameter_end_pc, &function.binding_sites)
}

pub(super) fn protected_positions(
    code: &[Instr],
    wide: &[WideInstruction],
    handlers: &[crate::bytecode::Handler],
    parameter_end_pc: u32,
    binding_sites: &[crate::bytecode::BindingSite],
) -> crate::bitset::BitSet {
    let mut protected = region_boundaries(code.len(), handlers, parameter_end_pc, binding_sites);
    for instruction in code {
        let instruction = widen(*instruction, wide);
        if instruction.op().immediate_role() == ImmediateRole::JumpTarget {
            protected.insert(instruction.jump_target() as usize);
        }
    }
    protected
}


/// Positions that open or close a handler or parameter region, or resume a binding site.
fn region_boundaries(
    length: usize,
    handlers: &[crate::bytecode::Handler],
    parameter_end_pc: u32,
    binding_sites: &[crate::bytecode::BindingSite],
) -> crate::bitset::BitSet {
    let mut protected = crate::bitset::BitSet::new(length + 1);
    for handler in handlers {
        protected.insert(handler.start as usize);
        protected.insert(handler.end as usize);
        protected.insert(handler.target as usize);
        if let Some(target) = handler.return_target {
            protected.insert(target as usize);
        }
    }
    if parameter_end_pc != 0 {
        protected.insert(parameter_end_pc as usize);
    }
    // A site's resume PC belongs to the preceding operation. Preserve this
    // boundary so fusion cannot merge that operation with its successor.
    for site in binding_sites {
        protected.insert(site.resume_pc as usize);
    }
    protected
}

fn rewrite_once(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &mut SiteTable<'_, FieldSite>,
    superinstructions: &[Superinstruction],
    current: &mut Option<CodeFacts>,
    scratch: &mut CompileScratch,
) -> bool {
    let rewrites_pair = |first: &Instr, second: &Instr| {
        !first.is_wide()
            && !second.is_wide()
            && !rules_for(first.op(), second.op()).is_empty()
    };
    // A pass without a rule-shaped pair changes nothing, and liveness is needed only to judge
    // pairs. A pair is an instruction and the next one that is not a `Nop`.
    let has_pair = (0..function.code.len()).any(|pc| {
        next_live(&function.code, pc)
            .is_some_and(|next| rewrites_pair(&function.code[pc], &function.code[next]))
    });
    if !has_pair {
        return false;
    }
    let Some(CodeLiveness { mut transfers, live, successors }) =
        CodeLiveness::of(current, function, methods, field_sites.as_slice(), superinstructions, scratch)
    else {
        return false;
    };
    let protected = protected_positions(
        &function.code,
        &function.wide,
        &function.handlers,
        function.parameter_end_pc,
        &function.binding_sites,
    );
    // A pair fuses in place: the replacement takes the first pc and the second becomes a `Nop`
    // that `remove_nops` drops once lowering ends. No pc moves, so the live sets, indexed by pc,
    // stay those of the code each pair is judged on.
    let mut rewritten = Vec::new();
    for index in 0..function.code.len() {
        let first = function.code[index];
        if first.op() == NOP || first.is_wide() {
            continue;
        }
        let Some(second_pc) = next_live(&function.code, index) else {
            continue;
        };
        let second = function.code[second_pc];
        // Every pc the pair swallows, the `Nop`s between included, must be a plain fallthrough.
        if second.is_wide() || (index + 1..=second_pc).any(|pc| protected.contains(pc)) {
            continue;
        }
        let rules = rules_for(first.op(), second.op());
        if rules.is_empty() {
            continue;
        }
        // The same control-flow facts justify frame roots and every removed register write.
        let original_definitions = liveness::definitions(first.as_wide(), superinstructions)
            | liveness::definitions(second.as_wide(), superinstructions);
        let live_out = liveness::live_out(&successors, &live, second_pc);
        let replacement = rules.iter().find_map(|&rule| {
            let replacement = apply_rule(
                rule,
                first,
                second,
                field_sites,
                &function.source_positions,
                index,
                second_pc,
            )?;
            let removed_definitions =
                original_definitions & !liveness::definitions(replacement.as_wide(), superinstructions);
            (live_out & removed_definitions == 0).then_some(replacement)
        });
        if let Some(instruction) = replacement {
            function.code[index] = instruction;
            function.code[second_pc] = Instr::new(NOP, 0, 0, 0, 0);
            rewritten.extend([index, second_pc]);
        }
    }
    if rewritten.is_empty() {
        *current = Some(CodeFacts::Live(CodeLiveness { transfers, live, successors }));
        return false;
    }
    drop(live);
    scratch.recycle_successors(successors);
    update_transfers(
        &mut transfers,
        &rewritten,
        function,
        methods,
        field_sites.as_slice(),
        superinstructions,
    );
    *current = Some(CodeFacts::Transfers(transfers));
    true
}

fn apply_rule(
    recipe: Recipe,
    first: Instr,
    second: Instr,
    field_sites: &mut SiteTable<'_, FieldSite>,
    source_positions: &crate::bytecode::SourcePositions,
    pc: usize,
    second_pc: usize,
) -> Option<Instr> {
    let replacement = apply_recipe(recipe, first, second, field_sites)?;
    let replacement = sound_replacement(first, second, replacement)?;
    if matches!(recipe, Recipe::GetSetThis)
        && let Some(crate::bytecode::FieldLookup::Site(index)) = replacement.field_lookup() {
            // The source offset in effect at a pc, found by the table's own lookup rather than a
            // decoded copy of the whole table.
            let at = |pc: usize| u32::try_from(pc).ok().and_then(|pc| source_positions.at(pc));
            let site = &mut field_sites[index];
            if site.sources[0] == FieldSite::NO_SOURCE {
                site.sources[0] = FieldSite::source_word(at(pc));
            }
            site.sources[2] = FieldSite::source_word(at(second_pc));
        }
    Some(replacement)
}

fn sound_replacement(first: Instr, second: Instr, replacement: Instr) -> Option<Instr> {
    replacement
        .effect()
        .contains(first.effect().union(second.effect()))
        .then_some(replacement)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn relocate(
    code: &mut [Instr],
    wide: &mut [WideInstruction],
    map: &[usize],
    handlers: &mut [crate::bytecode::Handler],
    parameter_end_pc: &mut u32,
    binding_sites: &mut [crate::bytecode::BindingSite],
    source_positions: &mut crate::bytecode::SourcePositions,
    old_code: &[Instr],
) {
    for instruction in code.iter_mut() {
        let decoded = widen(*instruction, wide);
        if decoded.op().immediate_role() != ImmediateRole::JumpTarget {
            continue;
        }
        // A wide instruction's target lives in the wide table, which each relocation visits
        // once: packed code refers to every wide entry at most once.
        if instruction.is_wide() {
            let entry = crate::bytecode::wide_entry_mut(wide, *instruction);
            entry.set_jump_target(map[entry.jump_target() as usize] as u32);
        } else {
            let target = map[instruction.jump_target() as usize] as u32;
            assert!(instruction.try_set_jump_target(target));
        }
    }
    for handler in handlers {
        handler.start = map[handler.start as usize] as u32;
        handler.end = map[handler.end as usize] as u32;
        handler.target = map[handler.target as usize] as u32;
        if let Some(target) = handler.return_target.as_mut() {
            *target = map[*target as usize] as u32;
        }
    }
    if *parameter_end_pc != 0 {
        *parameter_end_pc = map[*parameter_end_pc as usize] as u32;
    }
    for site in binding_sites {
        site.resume_pc = map[site.resume_pc as usize] as u32;
    }
    relocate_source_positions(code.len(), source_positions, old_code, wide, map);
}

fn relocate_source_positions(
    code_len: usize,
    source_positions: &mut crate::bytecode::SourcePositions,
    old_code: &[Instr],
    wide: &[WideInstruction],
    map: &[usize],
) {
    let relocated = relocated_source_positions(code_len, source_positions, old_code, wide, map);
    *source_positions = relocated.freeze();
}

/// The table `source_positions` becomes once the code is relocated through `map`. Surviving
/// entries stream in pc order, one ahead of the cursor, so no list of them is built.
fn relocated_source_positions(
    code_len: usize,
    source_positions: &crate::bytecode::SourcePositions,
    old_code: &[Instr],
    wide: &[WideInstruction],
    map: &[usize],
) -> crate::bytecode::SourcePositionsBuilder {
    let mut survivors = source_positions
        .iter()
        .filter(|position| source_instruction_survives(position.pc as usize, old_code, map))
        .peekable();
    // Relocation keeps or drops entries, so the old coding bounds the new one's.
    let mut relocated = crate::bytecode::SourcePositionsBuilder::with_capacity(source_positions.coded_len());
    let mut origin_pc = 0;
    let mut effective = None;
    let mut previous_location = None;

    for new_pc in 0..code_len {
        while origin_pc < old_code.len()
            && (!source_instruction_survives(origin_pc, old_code, map) || map[origin_pc] < new_pc)
        {
            origin_pc += 1;
        }
        // An instruction a rewrite inserted after its origin's first shares its position.
        if map.get(origin_pc) != Some(&new_pc) {
            continue;
        }
        let anchor = origin_pc;
        origin_pc += 1;
        while survivors
            .peek()
            .is_some_and(|position| position.pc as usize <= anchor)
        {
            effective = survivors.next();
        }
        let Some(entry) = effective else {
            continue;
        };
        let location = entry.offset;
        // Call anchors are observable even when their location matches a
        // preceding Move that a later pass may remove.
        if previous_location == Some(location)
            && widen(old_code[anchor], wide)
                .op()
                .control_flow_layout()
                != ControlFlowLayout::Call
        {
            continue;
        }
        relocated.push(crate::bytecode::SourceOffset {
            pc: new_pc as u32,
            offset: location,
        });
        previous_location = Some(location);
    }
    relocated
}

fn source_instruction_survives(old_pc: usize, old_code: &[Instr], map: &[usize]) -> bool {
    old_code[old_pc].op() != NOP || map[old_pc] != map[old_pc + 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditional_local_field_copy_keeps_valid_native_metadata() {
        let cases = [
            ("value", "const factory = () => ({model:42}); function Copy() { const result = factory(); if (result === null) {} this.model = result.model; } if (new Copy().model !== 42) throw Error('field copy failed');"),
            ("effects", r#"
                const events = [], poison = {};
                let fail = false;
                const factory = () => ({get model() { events.push('get'); if (fail) throw poison; return 42; }});
                function Copy() { const result = factory(); if (result === null) {} this.model = result.model; }
                Object.defineProperty(Copy.prototype, 'model', {set(value) { events.push('set:' + value); }});
                new Copy(); fail = true;
                try { new Copy(); throw Error('missing getter error'); } catch (error) { if (error !== poison) throw error; }
                if (events.join('|') !== 'get|set:42|get') throw Error('field copy reordered effects');
            "#),
        ];
        for (name, source) in cases {
            for generic in [false, true] {
                let program = if generic {
                    crate::Engine::specialize_unspecialized(source, "local-field-copy.js")
                } else {
                    crate::Engine::specialize(source, "local-field-copy.js")
                }.unwrap();
                assert!(program.validate().is_ok(), "{:?}", program.field_sites);
                let path = std::env::temp_dir().join(format!("quench-local-field-copy-{}-{name}-{generic}", std::process::id()));
                program.write_binary(&path).unwrap();
                let program = crate::ResidualProgram::read_binary(&path).unwrap();
                std::fs::remove_file(path).unwrap();
                for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
                    let mut runtime = crate::Runtime::new(crate::SystemHost);
                    runtime.set_execution_mode(mode);
                    runtime.execute(&program).unwrap();
                }
                {
                    let native = crate::NativeJsProgram::publish(program).unwrap();
                    let mut runtime = crate::Runtime::new(crate::SystemHost);
                    runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
                    runtime.execute_native(&native).unwrap();
                    assert_eq!(runtime.native_execution_rejections(), 0);
                }
            }
        }
    }

    #[test]
    fn local_field_keeps_the_pair_for_a_local_past_the_packed_field() {
        // Each captured binding and its function take a local, so the bindings declared after
        // them lie past what a packed field base holds; a field read of one (`hasOwn.call`)
        // must keep its load (Node oracle: no error).
        let bindings = (0..3000)
            .map(|index| format!("var c{index} = {index}; function f{index}() {{ return c{index}; }}\n"))
            .collect::<String>();
        let source = format!(
            r#"(function () {{ 'use strict';
{bindings}var hasOwn = Object.prototype.hasOwnProperty;
var side = {{ A: true }}, leap = {{ B: true }};
for (var type in leap) {{ if (hasOwn.call(leap, type)) side[type] = leap[type]; }}
if (Object.keys(side).join() !== 'A,B' || f1() !== 1) throw Error('field read');
}})();"#
        );
        let program = crate::Engine::specialize(&source, "local-field-wide.js").unwrap();
        crate::Runtime::new(crate::SystemHost).execute(&program).unwrap();
    }

    #[test]
    fn wide_jump_targets_are_protected_and_relocated() {
        for op in [Op::Jump, Op::JumpTrue, Op::JumpFalse] {
            let mut wide = vec![WideInstruction::new(op, 0, 0, 0, 3)];
            let old = vec![
                Instr::wide(0).unwrap(),
                Instr::new(Op::Nop, 0, 0, 0, 0),
                Instr::new(Op::Return, 0, 0, 0, 0),
                Instr::new(Op::Return, 0, 0, 0, 0),
            ];
            assert!(protected_positions(&old, &wide, &[], 0, &[]).contains(3));
            let mut code = vec![old[0], old[2], old[3]];
            let mut parameter_end = 0;
            relocate(
                &mut code,
                &mut wide,
                &[0, 1, 1, 2, 3],
                &mut [],
                &mut parameter_end,
                &mut [],
                &mut Default::default(),
                &old,
            );
            assert_eq!(wide[0].jump_target(), 2);
        }
    }

    #[test]
    fn wide_control_flow_keeps_producer_moves_inside_their_basic_block() {
        let mut program = Engine::specialize_unspecialized("", "wide-control-flow.js").unwrap();
        let function = &mut program.functions[0];
        function.registers = 6;
        function.wide = vec![WideInstruction::new(Op::JumpTrue, 0, 0, 0, 3)];
        function.code = vec![
            Instr::new(Op::LoadConst, 3, 0, 0, 0),
            Instr::wide(0).unwrap(),
            Instr::new(Op::Move, 5, 3, 0, 0),
            Instr::new(Op::Return, 5, 0, 0, 0),
        ];
        assert!(!coalesce_distant_producer_moves(function, &[] as &[MethodSiteSpec], &[], &[], &mut None, &mut CompileScratch::default()));
        assert_eq!(function.code[0].result_register(), 3);
        assert_eq!(function.code[2].op(), Op::Move);
    }

    #[test]
    fn constant_fusion_falls_back_when_tagged_operand_cannot_fit() {
        let index = (0..=u32::from(u16::MAX))
            .find(|&index| {
                Operand::constant(index)
                    .is_some_and(|operand| Instr::try_new(Op::Binary, 0, operand.0, 1, 0).is_none())
            })
            .unwrap();
        let load = Instr::new(Op::LoadConst, 0, 0, 0, index);
        let left = Instr::new(Op::Binary, 2, 0, 1, 0);
        let right = Instr::new(Op::Binary, 2, 1, 0, 0);
        let both = Instr::new(Op::Binary, 2, 0, 0, 0);
        for (recipe, binary) in [
            (Recipe::ConstantLeft, left),
            (Recipe::ConstantRight, right),
            (Recipe::ConstantLeft, both),
        ] {
            assert!(apply_recipe(recipe, load, binary, &mut SiteTable::default()).is_none());
        }
        let boundary = Instr::new(Op::LoadConst, 0, 0, 0, index - 1);
        assert!(apply_recipe(Recipe::ConstantLeft, boundary, left, &mut SiteTable::default()).is_some());
        assert!(apply_recipe(Recipe::ConstantRight, boundary, right, &mut SiteTable::default()).is_some());
    }

    #[test]
    fn rejects_rewrite_that_discards_heap_write() {
        let first = Instr::new(Op::SetField, 0, 1, 0, 0);
        let second = Instr::new(Op::Move, 2, 3, 0, 0);
        assert!(sound_replacement(first, second, second).is_none());
    }

    #[test]
    fn fuses_binary_condition_only_when_branch_consumes_result() {
        let binary = Instr::new(
            Op::Binary,
            7,
            Operand::register(2).0,
            Operand::constant(3).unwrap().0,
            6,
        );
        let branch = Instr::new(Op::JumpFalse, 7, 0, 0, 41);
        let fused = apply_recipe(Recipe::BinaryJumpFalse, binary, branch, &mut SiteTable::default()).unwrap();
        assert_eq!(fused.op(), Op::JumpBinaryFalse);
        assert_eq!(
            (fused.a(), fused.b(), fused.c(), fused.imm()),
            (6, binary.b(), binary.c(), 41)
        );

        let other = Instr::new(Op::JumpFalse, 8, 0, 0, 41);
        assert!(apply_recipe(Recipe::BinaryJumpFalse, binary, other, &mut SiteTable::default()).is_none());
    }

    #[test]
    fn inverts_a_branch_only_over_an_unreachable_forward_jump() {
        let ops = |code: &[Instr]| {
            code.iter()
                .map(|instruction| (instruction.op(), instruction.a(), instruction.imm()))
                .collect::<Vec<_>>()
        };
        let mut program = Engine::specialize_unspecialized("", "invert-branch.js").unwrap();
        let function = &mut program.functions[0];
        let move_ = Instr::new(Op::Move, 1, 0, 0, 0);
        function.code = vec![
            Instr::new(Op::JumpTrue, 0, 0, 0, 2),
            Instr::new(Op::Jump, 0, 0, 0, 4),
            move_,
            Instr::new(Op::JumpFalse, 0, 0, 0, 5),
            Instr::new(Op::Jump, 0, 0, 0, 0),
            move_,
        ];
        invert_branches_over_jumps(function);
        assert_eq!(
            ops(&function.code),
            [
                (Op::JumpFalse, 0, 4),
                (Op::Nop, 0, 0),
                (Op::Move, 1, 0),
                // The skipped jump is a back edge, so the branch keeps it.
                (Op::JumpFalse, 0, 5),
                (Op::Jump, 0, 0),
                (Op::Move, 1, 0),
            ]
        );

        // A jump some other branch also reaches stays.
        function.code = vec![
            Instr::new(Op::JumpFalse, 0, 0, 0, 2),
            Instr::new(Op::Jump, 0, 0, 0, 3),
            Instr::new(Op::JumpTrue, 0, 0, 0, 1),
            move_,
        ];
        let before = ops(&function.code);
        invert_branches_over_jumps(function);
        assert_eq!(ops(&function.code), before);
    }

    fn coalesce_fixture(
        code: Vec<Instr>,
        registers: u16,
        storage: &[Register],
    ) -> Vec<(Op, Register)> {
        let mut program = Engine::specialize_unspecialized("", "rewrite-coalesce.js").unwrap();
        let function = &mut program.functions[0];
        function.code = code;
        function.registers = registers;
        function.local_registers = storage
            .iter()
            .enumerate()
            .map(|(local, &register)| crate::bytecode::LocalRegister {
                local: local as u16,
                register,
            })
            .collect();
        while coalesce_distant_producer_moves(function, &[] as &[MethodSiteSpec], &[], &[], &mut None, &mut CompileScratch::default()) {}
        function
            .code
            .iter()
            .map(|instruction| (instruction.op(), instruction.a()))
            .collect()
    }

    fn sum_and_return(left: Register, right: Register, result: Register) -> [Instr; 2] {
        [
            Instr::new(
                Op::Binary,
                result,
                Operand::register(left).0,
                Operand::register(right).0,
                BinaryOperator::Addition as u32,
            ),
            Instr::new(Op::Return, result, 0, 0, 0),
        ]
    }

    #[test]
    fn coalesces_producers_into_later_overlapping_moves() {
        let mut code = vec![
            Instr::new(Op::LoadConst, 3, 0, 0, 0),
            Instr::new(Op::LoadConst, 4, 0, 0, 0),
            Instr::new(Op::Move, 5, 3, 0, 0),
            Instr::new(Op::Move, 6, 4, 0, 0),
        ];
        code.extend(sum_and_return(5, 6, 7));
        assert_eq!(
            coalesce_fixture(code, 8, &[]),
            [
                (Op::LoadConst, 5),
                (Op::LoadConst, 6),
                (Op::Nop, 0),
                (Op::Nop, 0),
                (Op::Binary, 7),
                (Op::Return, 7),
            ]
        );
    }

    #[test]
    fn keeps_moves_whose_span_or_registers_forbid_an_early_write() {
        let destination_read_in_span = vec![
            Instr::new(Op::LoadConst, 3, 0, 0, 0),
            Instr::new(Op::Move, 4, 5, 0, 0),
            Instr::new(Op::Move, 5, 3, 0, 0),
        ];
        let mut source_live_after = vec![
            Instr::new(Op::LoadConst, 3, 0, 0, 0),
            Instr::new(Op::LoadConst, 6, 0, 0, 0),
            Instr::new(Op::Move, 5, 3, 0, 0),
        ];
        source_live_after.extend(sum_and_return(5, 3, 7));
        let mut promoted_destination = vec![
            Instr::new(Op::LoadConst, 3, 0, 0, 0),
            Instr::new(Op::LoadConst, 6, 0, 0, 0),
            Instr::new(Op::Move, 5, 3, 0, 0),
        ];
        promoted_destination.extend(sum_and_return(5, 6, 7));
        for (code, storage) in [
            (destination_read_in_span, &[][..]),
            (source_live_after, &[][..]),
            (promoted_destination, &[5][..]),
        ] {
            let expected = code
                .iter()
                .map(|instruction| (instruction.op(), instruction.a()))
                .collect::<Vec<_>>();
            assert_eq!(coalesce_fixture(code, 8, storage), expected);
        }
    }

    #[test]
    fn producers_return_through_a_shared_return_inside_one_region() {
        let code = vec![
            Instr::new(Op::LoadConst, 3, 0, 0, 0),
            Instr::new(Op::JumpFalse, 0, 0, 0, 3),
            Instr::new(
                Op::Binary,
                3,
                Operand::register(1).0,
                Operand::register(2).0,
                BinaryOperator::Addition as u32,
            ),
            Instr::new(Op::Return, 3, 0, 0, 0),
        ];
        let mut program = Engine::specialize_unspecialized("", "rewrite-return.js").unwrap();
        let function = &mut program.functions[0];
        function.code = code.clone();
        return_through_shared_returns(function);
        assert!(function.code[2].returns_from_frame());
        assert_eq!(function.code[3].op(), Op::Return);
        function.code = code;
        function.parameter_end_pc = 3;
        return_through_shared_returns(function);
        assert!(!function.code[2].returns_from_frame());
    }

    fn rewrite_fixture(code: Vec<Instr>, registers: u16) -> Vec<Instr> {
        let mut program = Engine::specialize_unspecialized("", "rewrite-liveness.js").unwrap();
        let function = &mut program.functions[0];
        function.code = code;
        function.registers = registers;
        rewrite_once(function, &[] as &[MethodSiteSpec], &mut SiteTable::default(), &[], &mut None, &mut CompileScratch::default());
        remove_nops(function, &mut None, &mut CompileScratch::default());
        std::mem::take(&mut function.code)
    }

    fn assert_rewrite_preserves_code(code: Vec<Instr>, registers: u16) {
        let actual = rewrite_fixture(code.clone(), registers);
        assert_eq!(actual.len(), code.len());
        for (actual, expected) in actual.into_iter().zip(code) {
            assert_eq!(
                (
                    actual.op(),
                    actual.a(),
                    actual.b(),
                    actual.c(),
                    actual.imm()
                ),
                (
                    expected.op(),
                    expected.a(),
                    expected.b(),
                    expected.c(),
                    expected.imm()
                ),
            );
        }
    }

    #[test]
    fn producer_move_preserves_a_result_read_on_a_backward_edge() {
        let code = vec![
            Instr::new(Op::Jump, 0, 0, 0, 2),
            Instr::new(Op::Return, 3, 0, 0, 0),
            Instr::new(Op::LoadLocal, 3, 0, 0, 0),
            Instr::new(Op::Move, 4, 3, 0, 0),
            Instr::new(Op::Jump, 0, 0, 0, 1),
        ];
        assert_rewrite_preserves_code(code, 5);
    }

    #[test]
    fn binary_branch_discards_a_result_only_when_dead_on_every_successor() {
        let binary = Instr::new(
            Op::Binary,
            3,
            Operand::register(0).0,
            Operand::register(1).0,
            BinaryOperator::StrictEquality as u32,
        );
        let branch = Instr::new(Op::JumpFalse, 3, 0, 0, 3);
        let returned = Instr::new(Op::Return, 2, 0, 0, 0);
        let live_branch = vec![binary, branch, returned, Instr::new(Op::Return, 3, 0, 0, 0)];
        assert_rewrite_preserves_code(live_branch, 4);
        let dead_branch = vec![binary, branch, returned, returned];
        let fused = rewrite_fixture(dead_branch.clone(), 4);
        assert_eq!(fused[0].op(), Op::JumpBinaryFalse);
        assert_eq!(fused[0].jump_target(), 2);
        assert_eq!(fused.len(), dead_branch.len() - 1);
        assert_rewrite_preserves_code(dead_branch, super::super::liveness::ROOT_MAP_REGISTERS + 1);
    }

    #[test]
    fn constant_fusion_keeps_a_register_read_after_the_binary() {
        let code = vec![
            Instr::new(Op::LoadConst, 3, 0, 0, 0),
            Instr::new(
                Op::Binary,
                4,
                Operand::register(3).0,
                Operand::register(1).0,
                BinaryOperator::StrictEquality as u32,
            ),
            Instr::new(Op::Return, 3, 0, 0, 0),
        ];
        assert_rewrite_preserves_code(code, 5);
    }

    #[test]
    fn role_constants_name_the_rows_they_derive() {
        assert_eq!(NOP, Op::Nop);
        assert_eq!(MOVE, Op::Move);
        assert_eq!(RETURN, Op::Return);
        assert_eq!(JUMP, Op::Jump);
        assert_eq!(JUMP_BINARY_FALSE, Op::JumpBinaryFalse);
        assert_eq!(SUPER_CONST_ARRAY_OBJECT2, Op::SuperConstArrayObject2);
        assert_eq!(CONST_ARRAY_OBJECT2, [Op::MakeConstArray, Op::Binary, Op::Binary, Op::MakeObject2]);
        assert_eq!(NEGATED_JUMPS[Op::JumpTrue as usize], Some(Op::JumpFalse));
        assert_eq!(NEGATED_JUMPS[Op::JumpFalse as usize], Some(Op::JumpTrue));
        assert_eq!(NEGATED_JUMPS[Op::Jump as usize], None);
        assert_eq!(STRICT_VARIANTS[Op::SetField as usize], Some(Op::SetFieldStrict));
        assert_eq!(STRICT_VARIANTS[Op::SetThisField as usize], Some(Op::SetThisFieldStrict));
        assert_eq!(STRICT_VARIANTS[Op::Move as usize], None);
    }

    #[test]
    fn derived_rules_follow_row_properties_in_recipe_order() {
        let recipes = |first: Op, second: Op| rules_for(first, second).iter().copied().collect::<Vec<_>>();
        // A constant producer fuses into any op with operand B and C slots, left before right.
        for second in [Op::Binary, Op::NumericAdd, Op::NumericMultiply, Op::JumpBinaryFalse] {
            assert!(matches!(recipes(Op::LoadConst, second).as_slice(), [Recipe::ConstantLeft, Recipe::ConstantRight]));
        }
        // A `GetIndex` has index operands, not operands, so no constant reaches it.
        assert!(recipes(Op::LoadConst, Op::GetIndex).is_empty());
        assert!(matches!(recipes(Op::LoadLocal, Op::GetField).as_slice(), [Recipe::LocalField]));
        assert!(matches!(recipes(Op::LoadLocalPlain, Op::GetField).as_slice(), [Recipe::LocalField]));
        assert!(recipes(Op::LoadEnvLocal, Op::GetField).is_empty());
        assert!(matches!(recipes(Op::Binary, Op::Move).as_slice(), [Recipe::ProducerMove]));
        assert!(matches!(recipes(Op::Binary, Op::Return).as_slice(), [Recipe::ReturnResult]));
        // A frame local store fuses into a frame local load of the same slot, plain or not. Env stores and
        // env loads name another slot space, so they keep their pair.
        assert!(matches!(recipes(Op::StoreLocal, Op::LoadLocal).as_slice(), [Recipe::StoreLoadLocal]));
        assert!(matches!(recipes(Op::StoreLocalPlain, Op::LoadLocalPlain).as_slice(), [Recipe::StoreLoadLocal]));
        assert!(matches!(recipes(Op::StoreLocal, Op::LoadLocalPlain).as_slice(), [Recipe::StoreLoadLocal]));
        assert!(recipes(Op::StoreEnvLocal, Op::LoadLocal).is_empty());
        assert!(recipes(Op::StoreLocal, Op::LoadEnvLocal).is_empty());
        // A binary result fuses into a false jump only: `JumpTrue` jumps on the other test, and the
        // arithmetic rows name their operator in another immediate role.
        assert!(matches!(recipes(Op::Binary, Op::JumpFalse).as_slice(), [Recipe::BinaryJumpFalse]));
        assert!(recipes(Op::Binary, Op::JumpTrue).is_empty());
        assert!(recipes(Op::NumericAdd, Op::JumpFalse).is_empty());
        // A field read's result fuses into a `this` write's sink. `SetThisFieldStrict` is untagged.
        assert!(matches!(recipes(Op::GetField, Op::SetThisField).as_slice(), [Recipe::GetSetThis]));
        assert!(recipes(Op::GetField, Op::SetThisFieldStrict).is_empty());
        assert!(recipes(Op::Binary, Op::SetThisField).is_empty());
        // An untagged first op keeps its pair with a local load.
        assert!(recipes(Op::Nop, Op::LoadLocal).is_empty());
    }

    #[test]
    fn source_positions_follow_compaction_and_skip_removed_instructions() {
        use crate::bytecode::SourceOffset;

        let mut code = vec![Instr::new(Op::Return, 0, 0, 0, 0); 2];
        let old_code = vec![
            Instr::new(Op::Binary, 0, 0, 0, 0),
            Instr::new(Op::Return, 0, 0, 0, 0),
            Instr::new(Op::Nop, 0, 0, 0, 0),
            Instr::new(Op::Return, 0, 0, 0, 0),
        ];
        let mut source_positions: crate::bytecode::SourcePositions = vec![
            SourceOffset { pc: 0, offset: 0 },
            SourceOffset { pc: 1, offset: 4 },
            SourceOffset { pc: 2, offset: 8 },
        ]
        .into_iter()
        .collect();
        let map = [0, 0, 1, 1, 2];
        let mut parameter_end_pc = 0;
        let mut handlers = vec![];
        let mut binding_sites = vec![];

        relocate(
            &mut code,
            &mut [],
            &map,
            &mut handlers,
            &mut parameter_end_pc,
            &mut binding_sites,
            &mut source_positions,
            &old_code,
        );

        assert_eq!(
            source_positions.iter().collect::<Vec<_>>(),
            vec![
                SourceOffset { pc: 0, offset: 0 },
                SourceOffset { pc: 1, offset: 4 },
            ]
        );
    }

    #[test]
    fn wide_call_source_anchors_survive_later_argument_move_removal() {
        use crate::bytecode::SourceOffset;
        let wide = [WideInstruction::new(Op::Call, 0, 0, 0, 0)];
        let load = Instr::new(Op::LoadConst, 0, 0, 0, 0);
        let call = Instr::wide(0).unwrap();
        let mut positions = vec![
            SourceOffset { pc: 0, offset: 73 },
            SourceOffset { pc: 1, offset: 68 },
            SourceOffset { pc: 2, offset: 68 },
        ]
        .into_iter()
        .collect();
        let first = [load, Instr::new(Op::Move, 0, 0, 0, 0), call];
        relocate_source_positions(3, &mut positions, &first, &wide, &[0, 1, 2, 3]);
        assert_eq!(positions.last().unwrap().pc, 2);
        let second = [load, Instr::new(Op::Nop, 0, 0, 0, 0), call];
        relocate_source_positions(2, &mut positions, &second, &wide, &[0, 1, 1, 2]);
        assert_eq!(positions.iter().collect::<Vec<_>>(), vec![
            SourceOffset { pc: 0, offset: 73 },
            SourceOffset { pc: 1, offset: 68 },
        ]);
    }
}
