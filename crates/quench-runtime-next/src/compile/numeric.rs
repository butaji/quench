use crate::bytecode::{
    Function, Instr, NUMERIC_LOCAL_INC_STORE, NUMERIC_LOCAL_TARGET, Op, REGISTER_MASK,
    RETURN_REGISTER, specialized_numeric_op,
};

use super::rewrite::{protected_positions, relocate};

type Rule = fn(&[Instr]) -> Option<(u16, u16)>;

struct CompactRule {
    pattern: [Op; 2],
    replacement: fn(Instr, Instr, u64) -> Option<Instr>,
}

struct TripleCompactRule {
    pattern: [Op; 3],
    replacement: fn(Instr, Instr, Instr, u64) -> Option<Instr>,
}

const RULES: &[Rule] = &[local_inc_store];
const COMPACT_RULES: &[CompactRule] = &[CompactRule {
    pattern: [Op::Binary, Op::StoreLocal],
    replacement: binary_local_target,
}];
const TRIPLE_COMPACT_RULES: &[TripleCompactRule] = &[TripleCompactRule {
    pattern: [Op::LoadLocal, Op::LoadLocal, Op::GetIndex],
    replacement: local_index_sources,
}];

pub(super) fn apply(function: &mut Function, live: Option<&[u64]>) {
    if let Some(live) = live {
        compact_binary_stores(function, live);
    }
    for pc in 0..function.code.len() {
        if let Some((metadata, marker)) = RULES.iter().find_map(|rule| rule(&function.code[pc..])) {
            function.code[pc].set_b(metadata);
            function.code[pc].set_c(marker);
        }
    }
    for instruction in &mut function.code {
        if instruction.op() == Op::Binary
            && let Some(op) = specialized_numeric_op(instruction.imm())
        {
            instruction.set_op(op);
        }
    }
}

fn compact_binary_stores(function: &mut Function, live: &[u64]) {
    let old = std::mem::take(&mut function.code);
    let protected = protected_positions(&old, &function.handlers);
    let mut code = Vec::with_capacity(old.len());
    let mut map = vec![0; old.len() + 1];
    let mut pc = 0;
    while pc < old.len() {
        map[pc] = code.len();
        let triple = old
            .get(pc..pc + 3)
            .filter(|_| !protected[pc + 1] && !protected[pc + 2])
            .and_then(|window| {
                let live_after = live.get(pc + 3).copied().unwrap_or(u64::MAX);
                TRIPLE_COMPACT_RULES
                    .iter()
                    .filter(|rule| rule.pattern == [window[0].op(), window[1].op(), window[2].op()])
                    .find_map(|rule| {
                        (rule.replacement)(window[0], window[1], window[2], live_after)
                    })
            });
        if let Some(replacement) = triple {
            map[pc + 1] = code.len();
            map[pc + 2] = code.len();
            code.push(replacement);
            pc += 3;
            continue;
        }
        let replacement = old
            .get(pc + 1)
            .filter(|_| !protected[pc + 1])
            .and_then(|second| {
                let first = old[pc];
                let live_after = live.get(pc + 2).copied().unwrap_or(u64::MAX);
                COMPACT_RULES
                    .iter()
                    .filter(|rule| rule.pattern == [first.op(), second.op()])
                    .find_map(|rule| (rule.replacement)(first, *second, live_after))
            });
        if let Some(replacement) = replacement {
            map[pc + 1] = code.len();
            code.push(replacement);
            pc += 2;
        } else {
            code.push(old[pc]);
            pc += 1;
        }
    }
    map[old.len()] = code.len();
    relocate(&mut code, &map, &mut function.handlers);
    function.code = code;
}

fn local_index_sources(
    first: Instr,
    second: Instr,
    mut index: Instr,
    live_after: u64,
) -> Option<Instr> {
    (first.a() <= REGISTER_MASK
        && second.a() <= REGISTER_MASK
        && index.b() == first.a()
        && index.c() == second.a()
        && first.imm() <= u32::from(REGISTER_MASK)
        && second.imm() <= u32::from(REGISTER_MASK)
        && live_after & ((1 << first.a()) | (1 << second.a())) == 0)
        .then(|| {
            index.set_b(crate::bytecode::Operand::local(first.imm() as u16).0);
            index.set_c(crate::bytecode::Operand::local(second.imm() as u16).0);
            index
        })
}

fn binary_local_target(mut binary: Instr, store: Instr, live_after: u64) -> Option<Instr> {
    (binary.a() <= REGISTER_MASK
        && store.a() == binary.a()
        && store.b() == 0
        && store.imm() <= u32::from(REGISTER_MASK)
        && live_after & (1 << binary.a()) == 0)
        .then(|| {
            binary.set_a(NUMERIC_LOCAL_TARGET | store.imm() as u16);
            binary
        })
}

fn local_inc_store(code: &[Instr]) -> Option<(u16, u16)> {
    let [load, update, store, ..] = code else {
        return None;
    };
    (load.op() == Op::LoadLocal
        && update.op() == Op::IncDec
        && update.b() == load.a()
        && update.a() <= REGISTER_MASK
        && update.imm() <= 1
        && store.op() == Op::StoreLocal
        && store.a() == update.a()
        && store.b() == 0
        && store.imm() == load.imm())
    .then_some((
        update.a()
            | if update.imm() == 0 {
                0
            } else {
                RETURN_REGISTER
            },
        NUMERIC_LOCAL_INC_STORE,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_update_fusion_is_a_data_rule() {
        let code = [
            Instr::new(Op::LoadLocal, 2, 0, 0, 4),
            Instr::new(Op::IncDec, 3, 2, 0, 0),
            Instr::new(Op::StoreLocal, 3, 0, 0, 4),
        ];
        assert_eq!(local_inc_store(&code), Some((3, NUMERIC_LOCAL_INC_STORE)));
        let mut mismatch = code;
        mismatch[2].set_a(2);
        assert_eq!(local_inc_store(&mismatch), None);
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
            params: 0,
            rest: false,
            locals: 4,
            code: code.clone(),
            wide: vec![],
            registers: 3,
            dispatch: crate::bytecode::DispatchClass::Numeric,
            handlers: vec![],
            register_root_offset: u32::MAX,
        };
        let mut dead = make_function();
        compact_binary_stores(&mut dead, &[0, 0, 0, 0]);
        assert_eq!(dead.code.len(), 2);
        assert_eq!(dead.code[0].a(), NUMERIC_LOCAL_TARGET | 3);

        let mut live = make_function();
        compact_binary_stores(&mut live, &[0, 0, 1 << 2, 0]);
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
            params: 0,
            rest: false,
            locals: 4,
            code: code.clone(),
            wide: vec![],
            registers: 3,
            dispatch: crate::bytecode::DispatchClass::Numeric,
            handlers: vec![],
            register_root_offset: u32::MAX,
        };
        let mut dead = make_function();
        compact_binary_stores(&mut dead, &[0, 0, 0, 1 << 2, 0]);
        assert_eq!(dead.code.len(), 2);
        assert_eq!(crate::bytecode::Operand(dead.code[0].b()).tag(), 3);
        assert_eq!(crate::bytecode::Operand(dead.code[0].c()).tag(), 3);

        let mut live = make_function();
        compact_binary_stores(&mut live, &[0, 0, 0, (1 << 0) | (1 << 2), 0]);
        assert_eq!(live.code.len(), 4);
    }
}
