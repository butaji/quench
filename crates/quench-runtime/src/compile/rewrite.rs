use super::*;
use crate::bytecode::{ImmediateRole, Superinstruction};

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
            fields: &mut Vec<FieldSite>,
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
        |first: Instr, mut second: Instr, _: &mut Vec<FieldSite>| {
            let result = first.result_register();
            let constant = Operand::constant(first.constant_index() as u32);
            if second.operand_b().register_index() != Some(result) { return None; }
            second.set_operand_b(constant);
            if second.operand_c().register_index() == Some(result) {
                second.set_operand_c(constant);
            }
            Some(second)
        };
    ConstantRight: [LoadConst, Binary] =>
        |first: Instr, mut second: Instr, _: &mut Vec<FieldSite>| {
            if second.operand_c().register_index() != Some(first.result_register()) { return None; }
            second.set_operand_c(Operand::constant(first.constant_index() as u32));
            Some(second)
        };
    StoreLoadLocal: [StoreLocal, LoadLocal] =>
        |mut first: Instr, second: Instr, _: &mut Vec<FieldSite>| {
            if first.optional_register_b().is_some() || first.local_slot() != second.local_slot() { return None; }
            first.set_optional_register_b(Some(second.result_register()));
            Some(first)
        };
    ProducerMove: [
        LoadConst, Move; LoadLocal, Move; LoadEnvLocal, Move; LoadCapture, Move;
        LoadName, Move; Binary, Move; Unary, Move; GetField, Move; IncDec, Move;
        Call, Move; CallKnown, Move; CallMethod, Move; CallThisMethod, Move; Construct, Move;
        LoadThis, Move; MakeObject2, Move; SuperConstArrayObject2, Move
    ] => |mut first: Instr, second: Instr, _: &mut Vec<FieldSite>| {
        if second.register_b() != first.result_register() { return None; }
        first.set_result_register(second.result_register());
        Some(first)
    };
    BinaryJumpFalse: [Binary, JumpFalse] =>
        |first: Instr, second: Instr, _: &mut Vec<FieldSite>| {
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
    ] => |mut first: Instr, second: Instr, _: &mut Vec<FieldSite>| {
        if first.result_register() != second.register_a() { return None; }
        first.set_returns_from_frame();
        Some(first)
    };
    GetSetThis: [GetField, SetThisField] =>
        |mut first: Instr, second: Instr, fields: &mut Vec<FieldSite>| {
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
    methods: &[MethodSiteSpec],
    field_sites: &mut Vec<FieldSite>,
    superinstructions: &mut Vec<Superinstruction>,
) {
    let input = function.code.len();
    thread_jumps(function);
    invert_branches_over_jumps(function);
    let mut changed_passes = 0;
    while rewrite_super_window(function, superinstructions) {
        changed_passes += 1;
    }
    while coalesce_distant_producer_moves(function, methods, field_sites, superinstructions) {
        changed_passes += 1;
    }
    while rewrite_once(function, methods, field_sites, superinstructions) {
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

/// Retargets each unconditional jump whose destination is another unconditional jump to the
/// end of that chain. Only `Jump` sources are threaded: it is the back-edge collection safepoint,
/// so every retargeted back edge still collects.
fn thread_jumps(function: &mut BcFunction) {
    let length = function.code.len();
    for pc in 0..length {
        let mut instruction = function.code[pc];
        if instruction.op() != Op::Jump {
            continue;
        }
        let mut target = instruction.jump_target();
        // A chain longer than the function is a cycle of jumps; any member is equivalent.
        for _ in 0..length {
            match function.code.get(target as usize) {
                Some(next) if next.op() == Op::Jump && next.jump_target() != target => {
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
        &function.handlers,
        function.parameter_end_pc,
        &function.binding_sites,
    );
    for pc in 0..function.code.len().saturating_sub(1) {
        let (branch, skipped) = (function.code[pc], function.code[pc + 1]);
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
    methods: &[MethodSiteSpec],
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> bool {
    let Some(live) = liveness::analyze(function, methods, field_sites, superinstructions) else {
        return false;
    };
    let protected = protected_positions(
        &function.code,
        &function.handlers,
        function.parameter_end_pc,
        &function.binding_sites,
    );
    let storage = function
        .local_registers
        .iter()
        .fold(0_u64, |mask, entry| mask | register_bit(entry.register));
    let mut claimed_until = 0;
    let mut changed = false;
    for target in 0..function.code.len() {
        let mov = function.code[target];
        if mov.op() != Op::Move || protected[target] {
            continue;
        }
        let (source, destination) = (mov.register_b(), mov.result_register());
        let pair = register_bit(source) | register_bit(destination);
        if source == destination
            || pair.count_ones() != 2
            || pair & storage != 0
            || liveness::live_out(function, &live, target, mov) & register_bit(source) != 0
        {
            continue;
        }
        let Some(producer) = (claimed_until..target).rev().find(|&pc| {
            let instruction = function.code[pc];
            (liveness::uses(instruction, methods, field_sites, superinstructions)
                | liveness::definitions(instruction, superinstructions))
                & pair
                != 0
        }) else {
            continue;
        };
        let instruction = function.code[producer];
        let span_is_block = (producer + 1..=target).all(|pc| !protected[pc])
            && (producer..target).all(|pc| {
                let op = function.code[pc].op();
                matches!(
                    op.control_flow_layout(),
                    crate::bytecode::ControlFlowLayout::Fallthrough
                        | crate::bytecode::ControlFlowLayout::Call
                ) && !function.code[pc].returns_from_frame()
            })
            && function.handlers.iter().all(|handler| {
                handler.end as usize <= producer || handler.start as usize > target
            });
        if !span_is_block
            || !is_move_producer(instruction.op())
            || instruction.result_register() != source
            || liveness::definitions(instruction, superinstructions) != register_bit(source)
            || liveness::uses(instruction, methods, field_sites, superinstructions) & pair != 0
        {
            continue;
        }
        let mut retargeted = instruction;
        retargeted.set_result_register(destination);
        function.code[producer] = retargeted;
        function.code[target] = Instr::new(Op::Nop, 0, 0, 0, 0);
        claimed_until = target + 1;
        changed = true;
    }
    changed
}

/// The producers `ProducerMove` may retarget.
fn is_move_producer(op: Op) -> bool {
    RULES.iter().any(|rule| {
        matches!(rule.recipe, Recipe::ProducerMove) && rule.pattern == [op, Op::Move]
    })
}

fn register_bit(register: Register) -> u64 {
    1_u64.checked_shl(u32::from(register)).unwrap_or(0)
}

struct SuperRule {
    pattern: &'static [Op],
    fuse: fn(&[Instr], &mut Vec<Superinstruction>) -> Option<Instr>,
}

const CONST_ARRAY_OBJECT2: [Op; 4] = [Op::MakeConstArray, Op::Binary, Op::Binary, Op::MakeObject2];

const SUPER_RULES: &[SuperRule] = &[SuperRule {
    pattern: &[Op::MakeConstArray, Op::Binary, Op::Binary, Op::MakeObject2],
    fuse: fuse_const_array_object2,
}];

fn rewrite_super_window(
    function: &mut BcFunction,
    superinstructions: &mut Vec<Superinstruction>,
) -> bool {
    let old = std::mem::take(&mut function.code);
    let protected = protected_positions(
        &old,
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
    superinstructions: &mut Vec<Superinstruction>,
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
    handlers: &[crate::bytecode::Handler],
    parameter_end_pc: u32,
    binding_sites: &[crate::bytecode::BindingSite],
) -> Vec<bool> {
    let mut protected = vec![false; code.len() + 1];
    for instruction in code {
        if instruction.op().immediate_role() == ImmediateRole::JumpTarget {
            protected[instruction.jump_target() as usize] = true;
        }
    }
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
    methods: &[MethodSiteSpec],
    field_sites: &mut Vec<FieldSite>,
    superinstructions: &[Superinstruction],
) -> bool {
    let live = liveness::analyze(function, methods, field_sites, superinstructions);
    let old = std::mem::take(&mut function.code);
    let protected = protected_positions(
        &old,
        &function.handlers,
        function.parameter_end_pc,
        &function.binding_sites,
    );
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
            .filter(|_| !protected[index + 1])
            .and_then(|second| {
                // The same control-flow facts justify frame roots and every
                // removed register write. An unavailable map keeps the pair.
                let live = live.as_ref()?;
                let first = old[index];
                let original_definitions = liveness::definitions(first, superinstructions)
                    | liveness::definitions(*second, superinstructions);
                let live_out = liveness::live_out(function, live, index + 1, *second);
                RULES
                    .iter()
                    .filter(|rule| rule.pattern == [first.op(), second.op()])
                    .find_map(|rule| {
                        let replacement = apply_rule(*rule, first, *second, field_sites)?;
                        let removed_definitions = original_definitions
                            & !liveness::definitions(replacement, superinstructions);
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
    relocate(
        &mut code,
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

fn apply_rule(
    rule: Rule,
    first: Instr,
    second: Instr,
    field_sites: &mut Vec<FieldSite>,
) -> Option<Instr> {
    let replacement = apply_recipe(rule.recipe, first, second, field_sites)?;
    sound_replacement(first, second, replacement)
}

fn sound_replacement(first: Instr, second: Instr, replacement: Instr) -> Option<Instr> {
    replacement
        .effect()
        .contains(first.effect().union(second.effect()))
        .then_some(replacement)
}

pub(super) fn relocate(
    code: &mut [Instr],
    map: &[usize],
    handlers: &mut [crate::bytecode::Handler],
    parameter_end_pc: &mut u32,
    binding_sites: &mut [crate::bytecode::BindingSite],
    source_positions: &mut Vec<crate::bytecode::SourcePosition>,
    old_code: &[Instr],
) {
    for instruction in code.iter_mut() {
        if instruction.op().immediate_role() == ImmediateRole::JumpTarget {
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
    relocate_source_positions(code.len(), source_positions, old_code, map);
}

fn relocate_source_positions(
    code_len: usize,
    source_positions: &mut Vec<crate::bytecode::SourcePosition>,
    old_code: &[Instr],
    map: &[usize],
) {
    let surviving_positions = source_positions
        .iter()
        .copied()
        .filter(|position| source_instruction_survives(position.pc as usize, old_code, map))
        .collect::<Vec<_>>();
    let mut relocated = Vec::with_capacity(surviving_positions.len());
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
        assert_eq!(
            map[origin_pc], new_pc,
            "compacted instruction has an origin"
        );
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
        if previous_location == Some(location) {
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
            Operand::constant(3).0,
            6,
        );
        let branch = Instr::new(Op::JumpFalse, 7, 0, 0, 41);
        let fused = apply_recipe(Recipe::BinaryJumpFalse, binary, branch, &mut vec![]).unwrap();
        assert_eq!(fused.op(), Op::JumpBinaryFalse);
        assert_eq!(
            (fused.a(), fused.b(), fused.c(), fused.imm()),
            (6, binary.b(), binary.c(), 41)
        );

        let other = Instr::new(Op::JumpFalse, 8, 0, 0, 41);
        assert!(apply_recipe(Recipe::BinaryJumpFalse, binary, other, &mut vec![]).is_none());
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
        while coalesce_distant_producer_moves(function, &[], &[], &[]) {}
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

    fn rewrite_fixture(code: Vec<Instr>, registers: u16) -> Vec<Instr> {
        let mut program = Engine::specialize_unspecialized("", "rewrite-liveness.js").unwrap();
        let function = &mut program.functions[0];
        function.code = code;
        function.registers = registers;
        rewrite_once(function, &[], &mut vec![], &[]);
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
        assert_rewrite_preserves_code(dead_branch, u64::BITS as u16 + 1);
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
        assert_eq!(RULES.len(), 33);
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
                Operand::constant(1).0,
                Operand::local(0).0,
                8,
            ),
            Instr::new(
                Op::Binary,
                3,
                Operand::register(2).0,
                Operand::constant(2).0,
                8,
            ),
            Instr::new(Op::MakeObject2, 4, 1, 3, 0),
        ];
        let mut sites = vec![];
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
        let mut source_positions = vec![
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
        ];
        let map = [0, 0, 1, 1, 2];
        let mut parameter_end_pc = 0;
        let mut handlers = vec![];
        let mut binding_sites = vec![];

        relocate(
            &mut code,
            &map,
            &mut handlers,
            &mut parameter_end_pc,
            &mut binding_sites,
            &mut source_positions,
            &old_code,
        );

        assert_eq!(
            source_positions,
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
}
