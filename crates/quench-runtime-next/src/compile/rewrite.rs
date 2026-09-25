use super::*;
use crate::bytecode::{REGISTER_MASK, Superinstruction};

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
            if Operand(second.b()).register_index() != Some(first.a()) { return None; }
            second.set_b(Operand::constant(first.imm()).0);
            if Operand(second.c()).register_index() == Some(first.a()) {
                second.set_c(Operand::constant(first.imm()).0);
            }
            Some(second)
        };
    ConstantRight: [LoadConst, Binary] =>
        |first: Instr, mut second: Instr, _: &mut Vec<FieldSite>| {
            if Operand(second.c()).register_index() != Some(first.a()) { return None; }
            second.set_c(Operand::constant(first.imm()).0);
            Some(second)
        };
    StoreLoadLocal: [StoreLocal, LoadLocal] =>
        |mut first: Instr, second: Instr, _: &mut Vec<FieldSite>| {
            if first.b() != 0 || first.imm() != second.imm() || second.a() == u16::MAX { return None; }
            first.set_b(second.a() + 1);
            Some(first)
        };
    ProducerMove: [
        LoadConst, Move; LoadLocal, Move; LoadEnvLocal, Move; LoadCapture, Move;
        LoadName, Move; Binary, Move; Unary, Move; GetField, Move
    ] => |mut first: Instr, second: Instr, _: &mut Vec<FieldSite>| {
        if first.a() > REGISTER_MASK || second.b() != first.a() { return None; }
        first.set_a(second.a());
        Some(first)
    };
    BinaryJumpFalse: [Binary, JumpFalse] =>
        |first: Instr, second: Instr, _: &mut Vec<FieldSite>| {
            if first.a() != second.a() || first.a() > REGISTER_MASK { return None; }
            Some(Instr::new(Op::JumpBinaryFalse, first.imm() as u16, first.b(), first.c(), second.imm()))
        };
    ReturnResult: [
        Binary, Return; GetField, Return; Call, Return; CallKnown, Return;
        CallMethod, Return; CallThisMethod, Return; Construct, Return;
        MakeObject2, Return; SuperConstArrayObject2, Return
    ] => |mut first: Instr, second: Instr, _: &mut Vec<FieldSite>| {
        if first.a() != second.a() || first.a() > REGISTER_MASK { return None; }
        first.set_returns_from_frame();
        Some(first)
    };
    GetSetThis: [GetField, SetThisField] =>
        |mut first: Instr, second: Instr, fields: &mut Vec<FieldSite>| {
            if first.a() != second.a() || first.a() > REGISTER_MASK { return None; }
            let sink = (second.imm(), second.c());
            if first.b() == FieldBase::NESTED {
                let site = fields.get_mut(first.imm() as usize)?;
                if site.sink.is_some() { return None; }
                site.sink = Some(sink);
            } else {
                let site = FieldSite {
                    base: FieldBase(first.b()), first: (first.imm(), first.c()),
                    second: None, sink: Some(sink),
                };
                first.set_imm(fields.len() as u32);
                first.set_b(FieldBase::NESTED);
                first.set_c(0);
                fields.push(site);
            }
            first.set_this_result();
            Some(first)
        };
}

pub(super) fn apply(
    function: &mut BcFunction,
    field_sites: &mut Vec<FieldSite>,
    superinstructions: &mut Vec<Superinstruction>,
) {
    let input = function.code.len();
    let mut changed_passes = 0;
    while rewrite_super_window(function, superinstructions) {
        changed_passes += 1;
    }
    while rewrite_once(function, field_sites) {
        changed_passes += 1;
    }
    if std::env::var_os("RQJ_REWRITE_STATS").is_some() {
        eprintln!(
            "{{\"kind\":\"rqj-rewrite\",\"input\":{input},\"output\":{},\"changed_passes\":{changed_passes},\"scans\":{}}}",
            function.code.len(),
            changed_passes + 1
        );
    }
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
    let protected = protected_positions(&old, &function.handlers, function.parameter_end_pc);
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
    );
    function.code = code;
    changed
}

fn fuse_const_array_object2(
    window: &[Instr],
    superinstructions: &mut Vec<Superinstruction>,
) -> Option<Instr> {
    let code: [Instr; 4] = window.try_into().ok()?;
    if code.map(|instruction| instruction.op()) != CONST_ARRAY_OBJECT2
        || code[0].a() > REGISTER_MASK
        || code[1].a() > REGISTER_MASK
        || code[2].a() > REGISTER_MASK
        || code[3].a() > REGISTER_MASK
        || !([code[3].b(), code[3].c()].contains(&code[0].a())
            && [code[3].b(), code[3].c()].contains(&code[2].a()))
    {
        return None;
    }
    let site = superinstructions.len() as u32;
    superinstructions.push(Superinstruction { code });
    Some(Instr::new(
        Op::SuperConstArrayObject2,
        code[3].a(),
        0,
        0,
        site,
    ))
}

pub(super) fn protected_positions(
    code: &[Instr],
    handlers: &[crate::bytecode::Handler],
    parameter_end_pc: u32,
) -> Vec<bool> {
    let mut protected = vec![false; code.len() + 1];
    for instruction in code {
        if matches!(
            instruction.op(),
            Op::Jump | Op::JumpFalse | Op::JumpBinaryFalse
        ) {
            protected[instruction.imm() as usize] = true;
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
    protected
}

fn rewrite_once(function: &mut BcFunction, field_sites: &mut Vec<FieldSite>) -> bool {
    let old = std::mem::take(&mut function.code);
    let protected = protected_positions(&old, &function.handlers, function.parameter_end_pc);
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
            .filter(|second| {
                second.op() != Op::Move
                    || !matches!(
                        old[index].op(),
                        Op::LoadConst
                            | Op::LoadLocal
                            | Op::LoadEnvLocal
                            | Op::LoadCapture
                            | Op::LoadName
                            | Op::LoadNameTypeof
                            | Op::Binary
                            | Op::Unary
                            | Op::GetField
                    )
                    || register_dead_in_suffix(old[index].a(), &old[index + 2..], field_sites)
            })
            .and_then(|second| {
                let first = old[index];
                RULES
                    .iter()
                    .filter(|rule| rule.pattern == [first.op(), second.op()])
                    .find_map(|rule| apply_rule(*rule, first, *second, field_sites))
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
    );
    function.code = code;
    changed
}

fn register_dead_in_suffix(register: Register, suffix: &[Instr], fields: &[FieldSite]) -> bool {
    suffix
        .iter()
        .all(|instruction| !reads_register(*instruction, register, fields))
}

fn reads_register(instruction: Instr, register: Register, fields: &[FieldSite]) -> bool {
    let operand = |raw| Operand(raw).register_index() == Some(register);
    let range = |base: Register, count: u16| register >= base && register < base + count;
    match instruction.op() {
        Op::StoreLocal | Op::StoreEnvLocal | Op::StoreCapture | Op::StoreName => {
            instruction.a() == register
        }
        Op::StoreResolvedName => instruction.a() == register || instruction.b() == register,
        Op::SetFunctionNameKey => instruction.a() == register || instruction.b() == register,
        Op::ResolveName | Op::DeleteName => false,
        Op::LoadResolvedName => instruction.b() == register,
        Op::LoadImportMeta => false,
        Op::GetField if instruction.b() == FieldBase::NESTED => {
            fields
                .get(instruction.imm() as usize)
                .and_then(|site| site.base.register_index())
                == Some(register)
        }
        Op::GetField => FieldBase(instruction.b()).register_index() == Some(register),
        Op::CheckPrivate => instruction.a() == register,
        Op::PrivateIn => instruction.b() == register,
        Op::GetIndex | Op::MakeObject2 => {
            instruction.b() == register || instruction.c() == register
        }
        Op::CopyDataProperties => {
            instruction.a() == register
                || instruction.b() == register
                || instruction.c() == register
        }
        Op::ToPropertyKey | Op::ToNumeric => instruction.b() == register,
        Op::SuperConstArrayObject2 => true,
        Op::SetField | Op::DefineField => {
            instruction.a() == register || instruction.b() == register
        }
        Op::DefineComputedField => {
            instruction.a() == register
                || instruction.b() == register
                || instruction.c() == register
        }
        Op::SetThisField => instruction.a() == register,
        Op::InitializeThis => instruction.a() == register,
        Op::YieldStar => {
            let (state, next_method) = instruction.register_pair();
            instruction.a() == register
                || instruction.b() == register
                || instruction.c() == register
                || state == register
                || next_method == register
        }
        Op::SetIndex => {
            instruction.a() == register
                || instruction.b() == register
                || instruction.c() == register
        }
        Op::DefineArrayElement => instruction.a() == register || instruction.b() == register,
        Op::Binary | Op::JumpBinaryFalse => operand(instruction.b()) || operand(instruction.c()),
        Op::IncDec | Op::Unary | Op::Move => instruction.b() == register,
        Op::JumpFalse | Op::Return | Op::Throw => instruction.a() == register,
        Op::Call | Op::CallDirectEvalArray => {
            let window = instruction.call_window();
            instruction.b() == register
                || instruction.c() == register
                || range(window.base, window.count)
        }
        Op::CallKnown => {
            let window = instruction.call_window();
            range(window.base, window.count)
        }
        Op::CallMethod | Op::CallThisMethod => true,
        Op::Construct => {
            let arguments = match instruction.construct_arguments() {
                crate::bytecode::ConstructArguments::Registers(window) => {
                    range(window.base, window.count)
                }
                crate::bytecode::ConstructArguments::Array(array) => array == register,
            };
            instruction.b() == register || arguments
        }
        _ => false,
    }
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
) {
    for instruction in code {
        if matches!(
            instruction.op(),
            Op::Jump | Op::JumpFalse | Op::JumpBinaryFalse
        ) {
            instruction.set_imm(map[instruction.imm() as usize] as u32);
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
    fn producer_move_requires_the_original_register_to_be_dead() {
        let live = [Instr::new(Op::Return, 3, 0, 0, 0)];
        assert!(!register_dead_in_suffix(3, &live, &[]));
        assert!(register_dead_in_suffix(2, &live, &[]));
    }

    #[test]
    fn recipe_schema_generates_ordered_pattern_rows() {
        assert_eq!(RULES.len(), 22);
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
}
