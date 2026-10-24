use super::*;
use crate::bytecode::{
    FieldLayout, ImmediateRole, InstructionField, Superinstruction, WideInstruction,
};
use super::liveness::{RegisterSet, register_bit};

#[derive(Clone, Copy)]
struct Rule {
    pattern: [Op; 2],
    recipe: Recipe,
}

macro_rules! fusion_recipes {
    ($($recipe:ident: [$($first:ident, $second:ident);+ $(;)?] => $handler:expr;)+) => {
        #[derive(Clone, Copy)]
        enum Recipe { $($recipe),+ }

        const RULES: &[Rule] = &[
            $($(Rule {
                pattern: [Op::$first, Op::$second], recipe: Recipe::$recipe,
            }),+),+
        ];

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
fusion_recipes! {
    ConstantLeft: [LoadConst, Binary] =>
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
    ConstantRight: [LoadConst, Binary] =>
        |first: Instr, mut second: Instr, _: &mut SiteTable<'_, FieldSite>| {
            if second.operand_c().register_index() != Some(first.result_register()) { return None; }
            second.set_operand_c(Operand::constant(first.constant_index() as u32)?)?;
            Some(second)
        };
    StoreLoadLocal: [StoreLocal, LoadLocal] =>
        |mut first: Instr, second: Instr, _: &mut SiteTable<'_, FieldSite>| {
            if first.optional_register_b().is_some() || first.local_slot() != second.local_slot() { return None; }
            first.set_optional_register_b(Some(second.result_register()));
            Some(first)
        };
    LocalField: [LoadLocal, GetField; LoadLocalPlain, GetField] =>
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
            second.set_b(crate::bytecode::FieldBase::local(local)?.0);
            Some(second)
        };
    ProducerMove: [
        LoadConst, Move; LoadLocal, Move; LoadEnvLocal, Move; LoadCapture, Move;
        LoadName, Move; Binary, Move; Unary, Move; GetField, Move; IncDec, Move;
        Call, Move; CallKnown, Move; CallMethod, Move; CallThisMethod, Move; Construct, Move;
        LoadThis, Move; MakeObject2, Move; SuperConstArrayObject2, Move
    ] => |mut first: Instr, second: Instr, _: &mut SiteTable<'_, FieldSite>| {
        if second.register_b() != first.result_register() { return None; }
        first.set_result_register(second.result_register());
        Some(first)
    };
    BinaryJumpFalse: [Binary, JumpFalse] =>
        |first: Instr, second: Instr, _: &mut SiteTable<'_, FieldSite>| {
            if first.result_register() != second.register_a() { return None; }
            Some(Instr::new(
                Op::JumpBinaryFalse,
                first.binary_operator() as u16,
                first.operand_b().0,
                first.operand_c().0,
                second.jump_target(),
            ))
        };
    ReturnResult: [
        Binary, Return; Unary, Return; GetField, Return; Call, Return; CallKnown, Return;
        CallMethod, Return; CallThisMethod, Return; Construct, Return;
        MakeObject2, Return; MakeObjectLiteral, Return; SuperConstArrayObject2, Return
    ] => |mut first: Instr, second: Instr, _: &mut SiteTable<'_, FieldSite>| {
        if first.result_register() != second.register_a() { return None; }
        first.set_returns_from_frame();
        Some(first)
    };
    GetSetThis: [GetField, SetThisField] =>
        |mut first: Instr, second: Instr, fields: &mut SiteTable<'_, FieldSite>| {
            if first.result_register() != second.register_a() { return None; }
            let sink = (second.atom_index(), second.cache_site_index());
            match first.field_lookup()? {
                crate::bytecode::FieldLookup::Site(index) => {
                    let site = fields.get_mut(index)?;
                    if site.sink.is_some() { return None; }
                    site.sink = Some(sink);
                }
                crate::bytecode::FieldLookup::Atom { atom, base, cache_site } => {
                    fields.push(FieldSite {
                        base,
                        first: (atom, cache_site),
                        second: None,
                        sink: Some(sink),
                        sources: [None; 3],
                    });
                    first.set_field_lookup_site(fields.len() - 1);
                }
            }
            first.set_this_result();
            Some(first)
        };
}

pub(super) fn apply(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &mut SiteTable<'_, FieldSite>,
    superinstructions: &mut SiteTable<'_, Superinstruction>,
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
    while coalesce_distant_producer_moves(function, methods, field_sites.as_slice(), superinstructions.as_slice(), &mut liveness) {
        changed_passes += 1;
    }
    if forward_move_sources(function, methods, field_sites.as_slice(), superinstructions.as_slice(), &mut liveness) {
        changed_passes += 1;
    }
    while rewrite_once(function, methods, field_sites, superinstructions.as_slice(), &mut liveness) {
        changed_passes += 1;
    }
    if std::env::var_os("QUENCH_REWRITE_STATS").is_some() {
        eprintln!(
            "{{\"kind\":\"quench-rewrite\",\"input\":{input},\"output\":{},\"changed_passes\":{changed_passes},\"scans\":{}}}",
            function.code.len(),
            changed_passes + 1
        );
    }
}

/// Coalesces the copies frame-register promotion leaves: each promoted load or store of a
/// local becomes a `Move` beside the instruction that produces or consumes its value. The
/// same producer and consumer coalescing as `apply` folds them away; dropping the `Nop`s a
/// round leaves makes the next copies adjacent to their consumers, until none changes.
pub(super) fn coalesce_promoted_moves(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
) {
    let mut liveness = None;
    loop {
        let mut changed = false;
        while coalesce_distant_producer_moves(function, methods, field_sites, superinstructions, &mut liveness) {
            changed = true;
        }
        changed |= forward_move_sources(function, methods, field_sites, superinstructions, &mut liveness);
        changed |= remove_dead_moves(function, methods, field_sites, superinstructions, &mut liveness);
        if !changed {
            return;
        }
        remove_nops(function, &mut liveness);
    }
}

/// Removes each `Move` whose destination no path reads before writing it again: the copy into
/// a temporary or a promoted local that coalescing left behind once its readers moved to the
/// source. A parameter's register is kept, as legacy `f.arguments` reflection reads it.
fn remove_dead_moves(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
    current: &mut Option<CodeFacts>,
) -> bool {
    if !function.code.iter().any(|instruction| instruction.op() == Op::Move) {
        return false;
    }
    let Some(analysis) = CodeLiveness::of(current, function, methods, field_sites, superinstructions)
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
    let parameter_storage = function
        .local_registers
        .iter()
        .filter(|entry| entry.local < function.params)
        .fold(0, |mask, entry| mask | register_bit(entry.register));
    let mut changed = Vec::new();
    for pc in 0..function.code.len() {
        let mov = function.code[pc];
        if mov.op() != Op::Move || mov.is_wide() || protected[pc] {
            continue;
        }
        let destination = register_bit(mov.result_register());
        if destination & parameter_storage != 0
            || liveness::live_out(function, &analysis.live, pc, mov.as_wide()) & destination != 0
        {
            continue;
        }
        function.code[pc] = Instr::new(Op::Nop, 0, 0, 0, 0);
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
/// that followed it. The transfer masks of the instructions that stay are handed on with them.
fn remove_nops(function: &mut BcFunction, liveness: &mut Option<CodeFacts>) {
    let mut transfers = match liveness.take() {
        Some(CodeFacts::Live(analysis)) => Some(analysis.transfers),
        Some(CodeFacts::Transfers(transfers)) => Some(transfers),
        None => None,
    };
    let old = std::mem::take(&mut function.code);
    let mut code = Vec::with_capacity(old.len());
    let mut map = vec![0usize; old.len() + 1];
    for (pc, &instruction) in old.iter().enumerate() {
        map[pc] = code.len();
        if instruction.op() != Op::Nop {
            if let Some(transfers) = &mut transfers {
                transfers[code.len()] = transfers[pc];
            }
            code.push(instruction);
        }
    }
    if let Some(mut transfers) = transfers {
        transfers.truncate(code.len());
        *liveness = Some(CodeFacts::Transfers(transfers));
    }
    map[old.len()] = code.len();
    relocate(
        &mut code,
        &mut function.wide,
        &map,
        &mut function.handlers,
        &mut function.parameter_end_pc,
        &mut function.binding_sites,
        &mut function.source_positions,
        &old,
    );
    function.code = code;
}

/// The liveness of a function's code as it stands: each instruction's transfer masks and the
/// registers live before it.
struct CodeLiveness {
    transfers: Vec<(RegisterSet, RegisterSet)>,
    live: Vec<RegisterSet>,
}

/// What a pass hands the next about the code as it stands. A pass that leaves the code
/// unchanged hands on its liveness; one that rewrites instructions in place, or only drops
/// `Nop`s, hands on the transfer masks, which depend on each instruction alone and are
/// updated for the instructions it changed, so the next pass derives only the live sets.
enum CodeFacts {
    Live(CodeLiveness),
    Transfers(Vec<(RegisterSet, RegisterSet)>),
}

impl CodeLiveness {
    /// The liveness `current` hands on or completes, otherwise derived from the code.
    fn of(
        current: &mut Option<CodeFacts>,
        function: &BcFunction,
        methods: &(impl liveness::MethodArguments + ?Sized),
        field_sites: &[FieldSite],
        superinstructions: &[Superinstruction],
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
            None => liveness::transfers(function, methods, field_sites, superinstructions)?,
        };
        let live = liveness::analyze_transfers(function, &transfers);
        Some(Self { transfers, live })
    }

    /// The liveness of the code after `coalesce_distant_producer_moves` coalesced `spans`. A
    /// span is a block (one entry, falling through, no handler) where nothing between the
    /// producer and the move touches either register, so only the live sets after the producer
    /// change: `src` is no longer read there and `dst` holds the value instead, live up to the
    /// move exactly when it is live after it. The producer's own live set is unchanged.
    fn coalesced(
        self,
        spans: &[CoalescedSpan],
        changed: &[usize],
        function: &BcFunction,
        methods: &(impl liveness::MethodArguments + ?Sized),
        field_sites: &[FieldSite],
        superinstructions: &[Superinstruction],
    ) -> CodeFacts {
        let mut live = self.live;
        for span in spans {
            for live in &mut live[span.producer + 1..=span.target] {
                *live = (*live & !span.source) | span.destination_out;
            }
        }
        let mut transfers = self.transfers;
        update_transfers(&mut transfers, changed, function, methods, field_sites, superinstructions);
        debug_assert!(
            liveness::analyze_transfers(function, &transfers) == live,
            "coalesced liveness matches the code"
        );
        CodeFacts::Live(Self { transfers, live })
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
        let instruction = fixed_width(function.code[pc], &function.wide);
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
        if instruction.op() != Op::Jump || instruction.is_wide() {
            continue;
        }
        let mut target = instruction.jump_target();
        // A chain longer than the function is a cycle of jumps; any member is equivalent.
        for _ in 0..length {
            match function.code.get(target as usize) {
                Some(next)
                    if next.op() == Op::Jump && !next.is_wide() && next.jump_target() != target =>
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
        let inverted = match branch.op() {
            Op::JumpTrue => Op::JumpFalse,
            Op::JumpFalse => Op::JumpTrue,
            _ => continue,
        };
        if protected[pc + 1]
            || skipped.op() != Op::Jump
            || branch.jump_target() as usize != pc + 2
            || skipped.jump_target() as usize <= pc + 1
        {
            continue;
        }
        let target = skipped.jump_target();
        function.code[pc] = Instr::new(inverted, branch.register_a(), 0, 0, target);
        function.code[pc + 1] = Instr::new(Op::Nop, 0, 0, 0, 0);
    }
}

/// Retargets a producer to the destination of a later `Move` of its result, the
/// non-adjacent form of `ProducerMove`: `src = producer; ...; dst = Move src` becomes
/// `dst = producer; ...; Nop`. Call arguments are evaluated before their window is
/// reserved, so each argument computed ahead of another argument's evaluation ends in such
/// a move. Writing `dst` earlier is unobservable when the span between the two is one
/// handler-free basic block that neither reads nor writes `dst` or `src`, `src` is dead
/// after the move, and neither register is promoted local storage (which legacy
/// `f.arguments` reflection reads). Spans coalesced in one pass are disjoint, so each
/// candidate's liveness facts are unaffected by the others.
fn coalesce_distant_producer_moves(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
    current: &mut Option<CodeFacts>,
) -> bool {
    // Liveness is the expensive part; a function without a `Move` has nothing to coalesce.
    if !function
        .code
        .iter()
        .any(|instruction| instruction.op() == Op::Move)
    {
        return false;
    }
    let Some(analysis) = CodeLiveness::of(current, function, methods, field_sites, superinstructions)
    else {
        return false;
    };
    let CodeLiveness { transfers, live } = &analysis;
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
        .fold(0, |mask, entry| mask | register_bit(entry.register));
    let mut claimed_until = 0;
    let mut changed = Vec::new();
    let mut spans = Some(Vec::new());
    for target in 0..function.code.len() {
        let mov = function.code[target];
        if mov.op() != Op::Move || mov.is_wide() || protected[target] {
            continue;
        }
        let (source, destination) = (mov.register_b(), mov.result_register());
        let pair = register_bit(source) | register_bit(destination);
        // A move of a register onto itself changes nothing anything could observe. Its read
        // may have kept the register live upstream, so the liveness is derived anew.
        if source == destination {
            function.code[target] = Instr::new(Op::Nop, 0, 0, 0, 0);
            changed.push(target);
            spans = None;
            continue;
        }
        if pair.count_ones() != 2
            || pair & storage != 0
            || liveness::live_out(function, &live, target, mov.as_wide()) & register_bit(source)
                != 0
        {
            continue;
        }
        // Spans coalesced earlier in this pass end before `claimed_until`, so the
        // instructions scanned here still have the masks recorded in `transfers`.
        let Some(producer) = (claimed_until..target).rev().find(|&pc| {
            let (read, written) = transfers[pc];
            (read | written) & pair != 0
        }) else {
            continue;
        };
        let instruction = function.code[producer];
        if instruction.is_wide() {
            continue;
        }
        let span_is_block = (producer + 1..=target).all(|pc| !protected[pc])
            && (producer..target).all(|pc| {
                let instruction = fixed_width(function.code[pc], &function.wide);
                let op = instruction.op();
                matches!(
                    op.control_flow_layout(),
                    crate::bytecode::ControlFlowLayout::Fallthrough
                        | crate::bytecode::ControlFlowLayout::Call
                ) && !instruction.returns_from_frame()
            })
            && function
                .handlers
                .iter()
                .all(|handler| handler.end as usize <= producer || handler.start as usize > target);
        if !span_is_block
            || !fuses(instruction.op(), Op::Move, |recipe| {
                matches!(recipe, Recipe::ProducerMove)
            })
            || instruction.result_register() != source
            || transfers[producer].1 != register_bit(source)
            || transfers[producer].0 & pair != 0
        {
            continue;
        }
        let mut retargeted = instruction;
        retargeted.set_result_register(destination);
        function.code[producer] = retargeted;
        function.code[target] = Instr::new(Op::Nop, 0, 0, 0, 0);
        claimed_until = target + 1;
        changed.extend([producer, target]);
        if let Some(spans) = &mut spans {
            spans.push(CoalescedSpan {
                producer,
                target,
                source: register_bit(source),
                destination_out: register_bit(destination) & live[target + 1],
            });
        }
    }
    match spans {
        Some(spans) if !changed.is_empty() => {
            *current = Some(analysis.coalesced(&spans, &changed, function, methods, field_sites, superinstructions));
            true
        }
        _ => hand_on(analysis, &changed, function, methods, field_sites, superinstructions, current),
    }
}

/// A `src = producer; ...; dst = Move src` that `coalesce_distant_producer_moves` turned into
/// `dst = producer; ...; Nop`, with the bit of `src` and the bit of `dst` if live after the move.
struct CoalescedSpan {
    producer: usize,
    target: usize,
    source: RegisterSet,
    destination_out: RegisterSet,
}

/// Lets the instruction after a `Move` read the move's source in place of its destination when
/// the move feeds only that instruction: `d = Move s; x(d)` becomes `Nop; x(s)`, the forward
/// counterpart of `coalesce_distant_producer_moves`. The consumer must be reachable only through
/// the move, `d` dead after it and not promoted local storage (whose writes legacy `f.arguments`
/// reflection reads), and neither instruction inside a handler region. Only register, operand
/// and field-base fields are rewritten; the consumer's use and definition masks confirm that the
/// rewrite trades `d` for `s` and changes nothing else it reads or writes.
fn forward_move_sources(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
    current: &mut Option<CodeFacts>,
) -> bool {
    if !function
        .code
        .iter()
        .any(|instruction| instruction.op() == Op::Move)
    {
        return false;
    }
    let Some(analysis) = CodeLiveness::of(current, function, methods, field_sites, superinstructions)
    else {
        return false;
    };
    let CodeLiveness { transfers, live } = &analysis;
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
        .fold(0, |mask, entry| mask | register_bit(entry.register));
    let in_handler = |pc: usize| {
        function
            .handlers
            .iter()
            .any(|handler| handler.start as usize <= pc && pc < handler.end as usize)
    };
    let mut changed = Vec::new();
    let mut pc = 0;
    while pc + 1 < function.code.len() {
        let (mov, consumer) = (function.code[pc], function.code[pc + 1]);
        let candidate = mov.op() == Op::Move && !mov.is_wide() && !consumer.is_wide();
        let rewritten = candidate
            .then(|| {
                let (source, destination) = (mov.register_b(), mov.result_register());
                let (read, written) = transfers[pc + 1];
                let viable = source != destination
                    && !protected[pc + 1]
                    && !in_handler(pc)
                    && !in_handler(pc + 1)
                    && register_bit(destination) & storage == 0
                    && read & register_bit(destination) != 0
                    && liveness::live_out(function, &live, pc + 1, consumer.as_wide())
                        & register_bit(destination)
                        == 0;
                let rewritten = substitute_register_read(consumer, destination, source)?;
                let expected = (read & !register_bit(destination)) | register_bit(source);
                (viable
                    && liveness::uses(rewritten.as_wide(), methods, field_sites, superinstructions)
                        == expected
                    && liveness::definitions(rewritten.as_wide(), superinstructions) == written)
                    .then_some(rewritten)
            })
            .flatten();
        if let Some(rewritten) = rewritten {
            function.code[pc] = Instr::new(Op::Nop, 0, 0, 0, 0);
            function.code[pc + 1] = rewritten;
            changed.extend([pc, pc + 1]);
            pc += 2;
        } else {
            pc += 1;
        }
    }
    hand_on(analysis, &changed, function, methods, field_sites, superinstructions, current)
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
    let boundaries = region_boundaries(
        function.code.len(),
        &function.handlers,
        function.parameter_end_pc,
        &[],
    );
    let decoded = function.instructions().collect::<Vec<_>>();
    for pc in 1..decoded.len() {
        let (producer, next) = (decoded[pc - 1], decoded[pc]);
        if boundaries[pc]
            || next.op() != Op::Return
            || !fuses(producer.op(), Op::Return, |recipe| {
                matches!(recipe, Recipe::ReturnResult)
            })
            || producer.result_flags() != 0
            || producer.result_register() != next.register_a()
        {
            continue;
        }
        let packed = &mut function.code[pc - 1];
        if packed.is_wide() {
            function.wide[packed.wide_index()].set_returns_from_frame();
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
    let strict_op = |op| match op {
        Op::SetField => Some(Op::SetFieldStrict),
        Op::SetThisField => Some(Op::SetThisFieldStrict),
        _ => None,
    };
    for packed in &mut function.code {
        if packed.is_wide() {
            let wide = &mut function.wide[packed.wide_index()];
            if let Some(op) = strict_op(wide.op()) {
                wide.set_op(op);
            }
        } else if let Some(op) = strict_op(packed.op()) {
            packed.set_op(op);
        }
    }
}

/// Whether a rule of the selected recipe fuses `[first, second]`.
fn fuses(first: Op, second: Op, recipe: impl Fn(Recipe) -> bool) -> bool {
    RULES
        .iter()
        .any(|rule| rule.pattern == [first, second] && recipe(rule.recipe))
}


struct SuperRule {
    pattern: &'static [Op],
    fuse: fn(&[Instr], &mut SiteTable<'_, Superinstruction>) -> Option<Instr>,
}

const CONST_ARRAY_OBJECT2: [Op; 4] = [Op::MakeConstArray, Op::Binary, Op::Binary, Op::MakeObject2];

const SUPER_RULES: &[SuperRule] = &[SuperRule {
    pattern: &[Op::MakeConstArray, Op::Binary, Op::Binary, Op::MakeObject2],
    fuse: fuse_const_array_object2,
}];

fn rewrite_super_window(
    function: &mut BcFunction,
    superinstructions: &mut SiteTable<'_, Superinstruction>,
) -> bool {
    let old = std::mem::take(&mut function.code);
    let protected = protected_positions(
        &old,
        &function.wide,
        &function.handlers,
        function.parameter_end_pc,
        &function.binding_sites,
    );
    let mut code = Vec::with_capacity(old.len());
    let mut map = vec![0; old.len() + 1];
    let mut index = 0;
    let mut changed = false;
    while index < old.len() {
        map[index] = code.len();
        let replacement = SUPER_RULES.iter().find_map(|rule| {
            let width = rule.pattern.len();
            let window = old.get(index..index + width)?;
            if (1..width).any(|offset| protected[index + offset])
                || window.iter().any(|instruction| instruction.is_wide())
                || window
                    .iter()
                    .map(|instruction| instruction.op())
                    .ne(rule.pattern.iter().copied())
            {
                return None;
            }
            (rule.fuse)(window, superinstructions).map(|instruction| (instruction, width))
        });
        if let Some((instruction, width)) = replacement {
            for offset in 1..width {
                map[index + offset] = code.len();
            }
            code.push(instruction);
            index += width;
            changed = true;
        } else {
            code.push(old[index]);
            index += 1;
        }
    }
    map[old.len()] = code.len();
    relocate(
        &mut code,
        &mut function.wide,
        &map,
        &mut function.handlers,
        &mut function.parameter_end_pc,
        &mut function.binding_sites,
        &mut function.source_positions,
        &old,
    );
    function.code = code;
    changed
}

fn fuse_const_array_object2(
    window: &[Instr],
    superinstructions: &mut SiteTable<'_, Superinstruction>,
) -> Option<Instr> {
    let code: [Instr; 4] = window.try_into().ok()?;
    if code.map(|instruction| instruction.op()) != CONST_ARRAY_OBJECT2 {
        return None;
    }
    let [array, _, second, object] = code;
    let object_inputs = [object.register_b(), object.register_c()];
    if code
        .iter()
        .any(|instruction| instruction.result_flags() != 0)
        || !object_inputs.contains(&array.result_register())
        || !object_inputs.contains(&second.result_register())
    {
        return None;
    }
    let site = superinstructions.len() as u32;
    superinstructions.push(Superinstruction::new(code));
    Some(Instr::new(
        Op::SuperConstArrayObject2,
        object.result_register(),
        0,
        0,
        site,
    ))
}

pub(super) fn protected_positions(
    code: &[Instr],
    wide: &[WideInstruction],
    handlers: &[crate::bytecode::Handler],
    parameter_end_pc: u32,
    binding_sites: &[crate::bytecode::BindingSite],
) -> Vec<bool> {
    let mut protected = region_boundaries(code.len(), handlers, parameter_end_pc, binding_sites);
    for instruction in code {
        let instruction = fixed_width(*instruction, wide);
        if instruction.op().immediate_role() == ImmediateRole::JumpTarget {
            protected[instruction.jump_target() as usize] = true;
        }
    }
    protected
}

/// `instruction` in fixed-width form: a wide one through the function's wide table.
pub(super) fn fixed_width(instruction: Instr, wide: &[WideInstruction]) -> WideInstruction {
    if instruction.is_wide() {
        wide[instruction.wide_index()]
    } else {
        instruction.as_wide()
    }
}

/// Positions that open or close a handler or parameter region, or resume a binding site.
fn region_boundaries(
    length: usize,
    handlers: &[crate::bytecode::Handler],
    parameter_end_pc: u32,
    binding_sites: &[crate::bytecode::BindingSite],
) -> Vec<bool> {
    let mut protected = vec![false; length + 1];
    for handler in handlers {
        protected[handler.start as usize] = true;
        protected[handler.end as usize] = true;
        protected[handler.target as usize] = true;
        if let Some(target) = handler.return_target {
            protected[target as usize] = true;
        }
    }
    if parameter_end_pc != 0 {
        protected[parameter_end_pc as usize] = true;
    }
    // A site's resume PC belongs to the preceding operation. Preserve this
    // boundary so fusion cannot merge that operation with its successor.
    for site in binding_sites {
        protected[site.resume_pc as usize] = true;
    }
    protected
}

fn rewrite_once(
    function: &mut BcFunction,
    methods: &(impl liveness::MethodArguments + ?Sized),
    field_sites: &mut SiteTable<'_, FieldSite>,
    superinstructions: &[Superinstruction],
    current: &mut Option<CodeFacts>,
) -> bool {
    let rewrites_pair = |first: &Instr, second: &Instr| {
        !first.is_wide()
            && !second.is_wide()
            && RULES.iter().any(|rule| rule.pattern == [first.op(), second.op()])
    };
    // A pass without a rule-shaped pair or a `Nop` changes nothing, and liveness is needed
    // only to judge pairs.
    let has_pair = function.code.windows(2).any(|pair| rewrites_pair(&pair[0], &pair[1]));
    if !has_pair && !function.code.iter().any(|instruction| instruction.op() == Op::Nop) {
        return false;
    }
    let analysis = has_pair
        .then(|| CodeLiveness::of(current, function, methods, field_sites.as_slice(), superinstructions))
        .flatten();
    let live = analysis.as_ref().map(|analysis| &analysis.live);
    let old = std::mem::take(&mut function.code);
    let protected = protected_positions(
        &old,
        &function.wide,
        &function.handlers,
        function.parameter_end_pc,
        &function.binding_sites,
    );
    // Decoded once for the rules that record their sources.
    let positions = function.source_positions.iter().collect::<Vec<_>>();
    let mut code = Vec::with_capacity(old.len());
    let mut map = vec![0usize; old.len() + 1];
    let mut index = 0;
    let mut changed = false;
    while index < old.len() {
        map[index] = code.len();
        if old[index].op() == Op::Nop {
            index += 1;
            changed = true;
            continue;
        }
        let rewritten = old
            .get(index + 1)
            .filter(|second| !protected[index + 1] && !old[index].is_wide() && !second.is_wide())
            .and_then(|second| {
                let first = old[index];
                let mut rules = RULES
                    .iter()
                    .filter(|rule| rule.pattern == [first.op(), second.op()])
                    .peekable();
                rules.peek()?;
                // The same control-flow facts justify frame roots and every
                // removed register write. An unavailable map keeps the pair.
                let live = live.as_ref()?;
                let original_definitions =
                    liveness::definitions(first.as_wide(), superinstructions)
                        | liveness::definitions(second.as_wide(), superinstructions);
                let live_out = liveness::live_out(function, live, index + 1, second.as_wide());
                rules.find_map(|rule| {
                    let replacement = apply_rule(*rule, first, *second, field_sites, &positions, index)?;
                    let removed_definitions = original_definitions
                        & !liveness::definitions(replacement.as_wide(), superinstructions);
                    (live_out & removed_definitions == 0).then_some(replacement)
                })
            });
        if let Some(instruction) = rewritten {
            map[index + 1] = code.len();
            code.push(instruction);
            index += 2;
            changed = true;
        } else {
            code.push(old[index]);
            index += 1;
        }
    }
    map[old.len()] = code.len();
    if !changed {
        function.code = old;
        if let Some(analysis) = analysis {
            *current = Some(CodeFacts::Live(analysis));
        }
        return false;
    }
    *current = None;
    relocate(
        &mut code,
        &mut function.wide,
        &map,
        &mut function.handlers,
        &mut function.parameter_end_pc,
        &mut function.binding_sites,
        &mut function.source_positions,
        &old,
    );
    function.code = code;
    true
}

fn apply_rule(
    rule: Rule,
    first: Instr,
    second: Instr,
    field_sites: &mut SiteTable<'_, FieldSite>,
    positions: &[crate::bytecode::SourcePosition],
    pc: usize,
) -> Option<Instr> {
    let replacement = apply_recipe(rule.recipe, first, second, field_sites)?;
    let replacement = sound_replacement(first, second, replacement)?;
    if matches!(rule.recipe, Recipe::GetSetThis) {
        if let Some(crate::bytecode::FieldLookup::Site(index)) = replacement.field_lookup() {
            let at = |pc: usize| positions.partition_point(|p| p.pc as usize <= pc)
                .checked_sub(1).map(|index| (positions[index].line, positions[index].column));
            let site = &mut field_sites[index];
            if site.sources[0].is_none() { site.sources[0] = at(pc); }
            site.sources[2] = at(pc + 1);
        }
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
        let decoded = fixed_width(*instruction, wide);
        if decoded.op().immediate_role() != ImmediateRole::JumpTarget {
            continue;
        }
        // A wide instruction's target lives in the wide table, which each relocation visits
        // once: packed code refers to every wide entry at most once.
        if instruction.is_wide() {
            let entry = &mut wide[instruction.wide_index()];
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
    let surviving_positions = source_positions
        .iter()
        .filter(|position| source_instruction_survives(position.pc as usize, old_code, map))
        .collect::<Vec<_>>();
    let mut relocated = crate::bytecode::SourcePositions::default();
    let mut position_index = 0;
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
        while surviving_positions
            .get(position_index)
            .is_some_and(|position| position.pc as usize <= anchor)
        {
            effective = surviving_positions.get(position_index).copied();
            position_index += 1;
        }
        let Some(position) = effective else {
            continue;
        };
        let location = (position.line, position.column);
        // Call anchors are observable even when their location matches a
        // preceding Move that a later pass may remove.
        if previous_location == Some(location)
            && fixed_width(old_code[anchor], wide).op().control_flow_layout()
                != crate::bytecode::ControlFlowLayout::Call
        {
            continue;
        }
        relocated.push(crate::bytecode::SourcePosition {
            pc: new_pc as u32,
            line: position.line,
            column: position.column,
        });
        previous_location = Some(location);
    }
    *source_positions = relocated;
}

fn source_instruction_survives(old_pc: usize, old_code: &[Instr], map: &[usize]) -> bool {
    old_code[old_pc].op() != Op::Nop || map[old_pc] != map[old_pc + 1]
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(protected_positions(&old, &wide, &[], 0, &[])[3]);
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
        assert!(!coalesce_distant_producer_moves(function, &[] as &[MethodSiteSpec], &[], &[], &mut None));
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
        while coalesce_distant_producer_moves(function, &[] as &[MethodSiteSpec], &[], &[], &mut None) {}
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
        rewrite_once(function, &[] as &[MethodSiteSpec], &mut SiteTable::default(), &[], &mut None);
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
    fn recipe_schema_generates_ordered_pattern_rows() {
        assert_eq!(RULES.len(), 35);
        assert_eq!(RULES[0].pattern, [Op::LoadConst, Op::Binary]);
        assert_eq!(RULES[1].pattern, [Op::LoadConst, Op::Binary]);
        assert!(matches!(RULES[0].recipe, Recipe::ConstantLeft));
        assert!(matches!(RULES[1].recipe, Recipe::ConstantRight));
    }

    #[test]
    fn fuses_local_four_instruction_window_as_data() {
        let code = [
            Instr::new(Op::MakeConstArray, 1, 3, 0, 7),
            Instr::new(
                Op::Binary,
                2,
                Operand::constant(1).unwrap().0,
                Operand::local(0).0,
                8,
            ),
            Instr::new(
                Op::Binary,
                3,
                Operand::register(2).0,
                Operand::constant(2).unwrap().0,
                8,
            ),
            Instr::new(Op::MakeObject2, 4, 1, 3, 0),
        ];
        let mut sites = SiteTable::default();
        let fused = fuse_const_array_object2(&code, &mut sites).unwrap();
        assert_eq!(fused.op(), Op::SuperConstArrayObject2);
        assert_eq!(
            sites[0].code.map(|instruction| instruction.op()),
            CONST_ARRAY_OBJECT2
        );
    }

    #[test]
    fn source_positions_follow_compaction_and_skip_removed_instructions() {
        use crate::bytecode::SourcePosition;

        let mut code = vec![Instr::new(Op::Return, 0, 0, 0, 0); 2];
        let old_code = vec![
            Instr::new(Op::Binary, 0, 0, 0, 0),
            Instr::new(Op::Return, 0, 0, 0, 0),
            Instr::new(Op::Nop, 0, 0, 0, 0),
            Instr::new(Op::Return, 0, 0, 0, 0),
        ];
        let mut source_positions: crate::bytecode::SourcePositions = vec![
            SourcePosition {
                pc: 0,
                line: 1,
                column: 1,
            },
            SourcePosition {
                pc: 1,
                line: 1,
                column: 5,
            },
            SourcePosition {
                pc: 2,
                line: 1,
                column: 9,
            },
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
                SourcePosition {
                    pc: 0,
                    line: 1,
                    column: 1,
                },
                SourcePosition {
                    pc: 1,
                    line: 1,
                    column: 5,
                },
            ]
        );
    }

    #[test]
    fn wide_call_source_anchors_survive_later_argument_move_removal() {
        use crate::bytecode::SourcePosition;
        let wide = [WideInstruction::new(Op::Call, 0, 0, 0, 0)];
        let load = Instr::new(Op::LoadConst, 0, 0, 0, 0);
        let call = Instr::wide(0).unwrap();
        let mut positions = vec![
            SourcePosition { pc: 0, line: 1, column: 74 },
            SourcePosition { pc: 1, line: 1, column: 69 },
            SourcePosition { pc: 2, line: 1, column: 69 },
        ]
        .into_iter()
        .collect();
        let first = [load, Instr::new(Op::Move, 0, 0, 0, 0), call];
        relocate_source_positions(3, &mut positions, &first, &wide, &[0, 1, 2, 3]);
        assert_eq!(positions.last().unwrap().pc, 2);
        let second = [load, Instr::new(Op::Nop, 0, 0, 0, 0), call];
        relocate_source_positions(2, &mut positions, &second, &wide, &[0, 1, 1, 2]);
        assert_eq!(positions.iter().collect::<Vec<_>>(), vec![
            SourcePosition { pc: 0, line: 1, column: 74 },
            SourcePosition { pc: 1, line: 1, column: 69 },
        ]);
    }

}
