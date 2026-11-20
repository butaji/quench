use super::liveness::{RegisterSet, register_dead};
use crate::bytecode::{
    Function, Instr, NumericLocalStoreTarget, Op, Operand, REGISTER_MASK, Traits, the_op, specialized_numeric_op,
};

use super::rewrite::{LOCAL_READERS, NOP, NUMERIC_INDEX_CONSUMERS, NUMERIC_LOCAL_PRODUCERS, OpSet, next_live, protected_positions};

const LOCAL_BINDING_STORE_OP: Op = the_op(Traits::LOCAL_BINDING_STORE);
const PLAIN_LOCAL_STORE_OP: Op = the_op(Traits::PLAIN_LOCAL_STORE);

type Rule = fn(&[Instr]) -> Option<NumericLocalStoreTarget>;

/// A pair whose first op is in `first` (a derived set, see `rewrite::OpSet`) and second is `second`.
struct CompactRule {
    first: &'static OpSet,
    second: Op,
    replacement: fn(Instr, Instr, RegisterSet) -> Option<Instr>,
}

/// A window whose ops are each in the matching derived set.
struct TripleCompactRule {
    pattern: [&'static OpSet; 3],
    replacement: fn(Instr, Instr, Instr, RegisterSet) -> Option<Instr>,
}

// The window rule `local_inc_store` stays a function: its ends must name the same local, and
// the `IncDec` update's `BooleanFlag` picks the direction. Both are dataflow, which no row states.
const RULES: &[Rule] = &[local_inc_store];
// `binary_local_target` needs the first op's row to admit a numeric local target. The store
// takes the binding-store role: `StoreEnvLocal` shares `StoreLocal`'s row fields but stores to an environment slot.
const NUMERIC_COMPACT_RULES: &[CompactRule] = &[CompactRule {
    first: &NUMERIC_LOCAL_PRODUCERS,
    second: LOCAL_BINDING_STORE_OP,
    replacement: binary_local_target,
}];
const PLAIN_LOCAL_COMPACT_RULES: &[CompactRule] = &[CompactRule {
    first: &NUMERIC_LOCAL_PRODUCERS,
    second: PLAIN_LOCAL_STORE_OP,
    replacement: binary_local_target,
}];
// `local_index_sources` reads two local operands into a `GetIndex`: both sources are local reads,
// and the index op's B and C are index operands.
const TRIPLE_COMPACT_RULES: &[TripleCompactRule] = &[TripleCompactRule {
    pattern: [&LOCAL_READERS, &LOCAL_READERS, &NUMERIC_INDEX_CONSUMERS],
    replacement: local_index_sources,
}];

pub(super) fn apply(function: &mut Function, live: Option<&[RegisterSet]>) {
    if let Some(live) = live {
        compact_binary_stores(function, live, NUMERIC_COMPACT_RULES, TRIPLE_COMPACT_RULES);
    }
    for pc in 0..function.code.len() {
        if function.code[pc].op() == NOP {
            continue;
        }
        // The rules read the live instructions from `pc`, the `Nop`s they fall through skipped.
        let window: smallvec::SmallVec<[Instr; 3]> = function.code[pc..]
            .iter()
            .copied()
            .filter(|instruction| instruction.op() != NOP)
            .take(3)
            .collect();
        if let Some(target) = RULES.iter().find_map(|rule| rule(&window[..])) {
            function.code[pc].set_numeric_local_store_target(Some(target));
        }
    }
    for instruction in &mut function.code {
        if instruction.op().traits().has(Traits::BINARY_RESULT)
            && let Some(op) = specialized_numeric_op(instruction.binary_operator())
        {
            instruction.set_op(op);
        }
    }
}

/// Compacts the plain local stores under `live`, and whether it rewrote any instruction.
pub(super) fn apply_plain_local_stores(function: &mut Function, live: Option<&[RegisterSet]>) -> bool {
    live.is_some_and(|live| compact_binary_stores(function, live, PLAIN_LOCAL_COMPACT_RULES, &[]))
}

fn compact_binary_stores(
    function: &mut Function,
    live: &[RegisterSet],
    rules: &[CompactRule],
    triple_rules: &[TripleCompactRule],
) -> bool {
    let protected = protected_positions(
        &function.code,
        &function.wide,
        &function.handlers,
        function.parameter_end_pc,
        &function.binding_sites,
    );
    let mut changed = false;
    let mut pc = 0;
    while pc < function.code.len() {
        let first = function.code[pc];
        if first.op() == NOP {
            pc += 1;
            continue;
        }
        // The window is the next instructions that are not `Nop`s, each fused one left in place
        // as a `Nop` by an earlier rewrite falling through.
        let second = next_live(&function.code, pc);
        let third = second.and_then(|second| next_live(&function.code, second));
        let triple_window = match (second, third) {
            (Some(second), Some(third)) if !(pc + 1..=third).any(|at| protected.contains(at)) => Some((second, third)),
            _ => None,
        };
        let triple = triple_window.and_then(|(second_pc, third_pc)| {
            let window = [first, function.code[second_pc], function.code[third_pc]];
            let live_after = live.get(third_pc + 1).copied().unwrap_or(RegisterSet::MAX);
            triple_rules
                .iter()
                .filter(|rule| rule.pattern.iter().zip(window.iter()).all(|(set, instruction)| set[instruction.op() as usize]))
                .find_map(|rule| (rule.replacement)(window[0], window[1], window[2], live_after))
                .map(|replacement| (replacement, second_pc, third_pc))
        });
        if let Some((replacement, second_pc, third_pc)) = triple {
            function.code[pc] = replacement;
            function.code[second_pc] = Instr::new(NOP, 0, 0, 0, 0);
            function.code[third_pc] = Instr::new(NOP, 0, 0, 0, 0);
            changed = true;
            pc = third_pc + 1;
            continue;
        }
        let pair = second
            .filter(|&second_pc| !(pc + 1..=second_pc).any(|at| protected.contains(at)))
            .and_then(|second_pc| {
                let second_instruction = function.code[second_pc];
                let live_after = live.get(second_pc + 1).copied().unwrap_or(RegisterSet::MAX);
                rules
                    .iter()
                    .filter(|rule| rule.first[first.op() as usize] && rule.second == second_instruction.op())
                    .find_map(|rule| (rule.replacement)(first, second_instruction, live_after))
                    .map(|replacement| (replacement, second_pc))
            });
        if let Some((replacement, second_pc)) = pair {
            function.code[pc] = replacement;
            function.code[second_pc] = Instr::new(NOP, 0, 0, 0, 0);
            changed = true;
            pc = second_pc + 1;
        } else {
            pc += 1;
        }
    }
    changed
}

fn local_index_sources(
    first: Instr,
    second: Instr,
    mut index: Instr,
    live_after: RegisterSet,
) -> Option<Instr> {
    (index.operand_b().register_index() == Some(first.result_register())
        && index.operand_c().register_index() == Some(second.result_register())
        && first.local_slot() <= usize::from(REGISTER_MASK)
        && second.local_slot() <= usize::from(REGISTER_MASK)
        && register_dead(live_after, usize::from(first.result_register()))
        && register_dead(live_after, usize::from(second.result_register())))
        .then_some(())?;
    index.set_operand_b(Operand::local(first.local_slot() as u16))?;
    index.set_operand_c(Operand::local(second.local_slot() as u16))?;
    Some(index)
}

fn binary_local_target(mut binary: Instr, store: Instr, live_after: RegisterSet) -> Option<Instr> {
    (store.register_a() == binary.result_register()
        && store.optional_register_b().is_none()
        && store.local_slot() <= usize::from(REGISTER_MASK)
        && register_dead(live_after, usize::from(binary.result_register())))
        .then(|| {
            binary.set_result_local_target(store.local_slot() as u16);
            binary
        })
}

fn local_inc_store(code: &[Instr]) -> Option<NumericLocalStoreTarget> {
    let [load, update, store, ..] = code else {
        return None;
    };
    (load.op().traits().has(Traits::LOCAL_BINDING_LOAD)
        && update.op().traits().has(Traits::INCREMENT_UPDATE)
        && update.register_b() == load.result_register()
        && update.boolean_flag().is_some()
        && store.op().traits().has(Traits::LOCAL_BINDING_STORE)
        && store.register_a() == update.result_register()
        && store.optional_register_b().is_none()
        && store.local_slot() == load.local_slot())
    .then(|| NumericLocalStoreTarget {
        register: update.result_register(),
        decrement: update.boolean_flag() == Some(true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::NUMERIC_LOCAL_TARGET;

    #[test]
    fn local_update_fusion_is_a_data_rule() {
        let code = [
            Instr::new(Op::LoadLocal, 2, 0, 0, 4),
            Instr::new(Op::IncDec, 3, 2, 0, 0),
            Instr::new(Op::StoreLocal, 3, 0, 0, 4),
        ];
        assert_eq!(
            local_inc_store(&code),
            Some(NumericLocalStoreTarget {
                register: 3,
                decrement: false,
            })
        );
        let mut mismatch = code;
        mismatch[2].set_a(2);
        assert_eq!(local_inc_store(&mismatch), None);
        // Nonmatching opcodes do not have the update operand layout. Their
        // fields must never be decoded as if the pattern had matched.
        for non_update in [Op::Binary, Op::StoreLocal, Op::Return] {
            let mut mismatch = code;
            mismatch[1] = Instr::new(non_update, 0, 0, 0, 0);
            assert_eq!(local_inc_store(&mismatch), None);
        }
    }

    #[test]
    fn binary_store_compaction_requires_a_dead_temporary() {
        let code = vec![
            Instr::new(Op::Binary, 2, 0, 1, 8),
            Instr::new(Op::StoreLocal, 2, 0, 0, 3),
            Instr::new(Op::Return, 0, 0, 0, 0),
        ];
        let make_function = || Function {
            parent: None,
            name: None,
            inferred_name: None,
            is_arrow: false,
            self_binding_slot: None,
            source_span: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![].into(),
            rare: None,
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            constant_fields: Default::default(),
            super_home_atom: None,
            constructible: true,
            class_initializer: false,
            parameter_eval_arguments_error: false,
            unaliased_arguments: false, rest_alias: false,
            prototype_unobservable: false,
            arguments_slot: None,
            simple_parameters: true,
            strict: false,
            locals: 4,
            local_atoms: vec![].into(),
            environment_atoms: vec![].into(),
            selective_capture_slots: None,
            declared_functions: Default::default(),
            declared_captures: Default::default(),
            local_registers: Vec::new().into(),
            inherited_with_scope: false,
            lexical_atoms: vec![].into(),
            name_bindings: Default::default(),
            binding_sites: vec![].into(),
            source_positions: Default::default(),
            environment_clones: vec![].into(),
            code: code.clone(),
            wide: vec![],
            registers: 3,
            dispatch: crate::bytecode::DispatchClass::Numeric,
            decoded: Default::default(),
            plain_locals: Default::default(), caches: Default::default(),
            lazy_flags: Default::default(),
            call_plan: Default::default(),
            handlers: vec![].into(),
            register_roots: Default::default(),
            undefined_register: None,
            parameter_registers: None,
            initial_register: Default::default(),
        };
        let mut dead = make_function();
        compact_binary_stores(
            &mut dead,
            &[crate::compile::liveness::RegisterSet::default(); 4],
            NUMERIC_COMPACT_RULES,
            TRIPLE_COMPACT_RULES,
        );
        crate::compile::rewrite::remove_nops(&mut dead, &mut None, &mut crate::compile::CompileScratch::default());
        assert_eq!(dead.code.len(), 2);
        assert_eq!(dead.code[0].a(), NUMERIC_LOCAL_TARGET | 3);

        let mut live = make_function();
        compact_binary_stores(
            &mut live,
            &[crate::compile::liveness::RegisterSet::default(), crate::compile::liveness::RegisterSet::default(), crate::compile::liveness::register_bit(2), crate::compile::liveness::RegisterSet::default()],
            NUMERIC_COMPACT_RULES,
            TRIPLE_COMPACT_RULES,
        );
        crate::compile::rewrite::remove_nops(&mut live, &mut None, &mut crate::compile::CompileScratch::default());
        assert_eq!(live.code.len(), 3);
    }

    #[test]
    fn local_index_compaction_requires_both_temporaries_dead() {
        let code = vec![
            Instr::new(Op::LoadLocal, 0, 0, 0, 2),
            Instr::new(Op::LoadLocal, 1, 0, 0, 3),
            Instr::new(Op::GetIndex, 2, 0, 1, 0),
            Instr::new(Op::Return, 2, 0, 0, 0),
        ];
        let make_function = || Function {
            parent: None,
            name: None,
            inferred_name: None,
            is_arrow: false,
            self_binding_slot: None,
            source_span: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![].into(),
            rare: None,
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            constant_fields: Default::default(),
            super_home_atom: None,
            constructible: true,
            class_initializer: false,
            parameter_eval_arguments_error: false,
            unaliased_arguments: false, rest_alias: false,
            prototype_unobservable: false,
            arguments_slot: None,
            simple_parameters: true,
            strict: false,
            locals: 4,
            local_atoms: vec![].into(),
            environment_atoms: vec![].into(),
            selective_capture_slots: None,
            declared_functions: Default::default(),
            declared_captures: Default::default(),
            local_registers: Vec::new().into(),
            inherited_with_scope: false,
            lexical_atoms: vec![].into(),
            name_bindings: Default::default(),
            binding_sites: vec![].into(),
            source_positions: Default::default(),
            environment_clones: vec![].into(),
            code: code.clone(),
            wide: vec![],
            registers: 3,
            dispatch: crate::bytecode::DispatchClass::Numeric,
            decoded: Default::default(),
            plain_locals: Default::default(), caches: Default::default(),
            lazy_flags: Default::default(),
            call_plan: Default::default(),
            handlers: vec![].into(),
            register_roots: Default::default(),
            undefined_register: None,
            parameter_registers: None,
            initial_register: Default::default(),
        };
        let mut dead = make_function();
        compact_binary_stores(
            &mut dead,
            &[crate::compile::liveness::RegisterSet::default(), crate::compile::liveness::RegisterSet::default(), crate::compile::liveness::RegisterSet::default(), crate::compile::liveness::register_bit(2), crate::compile::liveness::RegisterSet::default()],
            NUMERIC_COMPACT_RULES,
            TRIPLE_COMPACT_RULES,
        );
        crate::compile::rewrite::remove_nops(&mut dead, &mut None, &mut crate::compile::CompileScratch::default());
        assert_eq!(dead.code.len(), 2);
        assert_eq!(crate::bytecode::Operand(dead.code[0].b()).tag(), 3);
        assert_eq!(crate::bytecode::Operand(dead.code[0].c()).tag(), 3);

        let mut live = make_function();
        compact_binary_stores(
            &mut live,
            &[crate::compile::liveness::RegisterSet::default(), crate::compile::liveness::RegisterSet::default(), crate::compile::liveness::RegisterSet::default(), crate::compile::liveness::register_bit(0) | crate::compile::liveness::register_bit(2), crate::compile::liveness::RegisterSet::default()],
            NUMERIC_COMPACT_RULES,
            TRIPLE_COMPACT_RULES,
        );
        crate::compile::rewrite::remove_nops(&mut live, &mut None, &mut crate::compile::CompileScratch::default());
        assert_eq!(live.code.len(), 4);
    }
}
