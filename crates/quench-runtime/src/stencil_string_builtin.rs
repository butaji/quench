//! Guarded portable string-builtin regions over canonical residual operations.
//!
//! Static selection proves lowered wiring. Runtime admission snapshots an
//! owned primitive string from a plain own property before calling allocating
//! helpers, so no object borrow crosses reentry or allocation.

use crate::ir::Opcode;
use crate::machine::{BaselineEntry, CodeView};
use crate::value::Value;

const CASE_LEN: usize = 5;
const SEARCH_LEN: usize = 17;

#[derive(Clone, Debug, PartialEq)]
enum StringBuiltinRecipe {
    Case {
        receiver_slot: u16,
    },
    Search {
        receiver_slot: u16,
        needles: [Value; 3],
    },
}

pub(crate) struct StringBuiltinPlan {
    recipe: StringBuiltinRecipe,
}

impl StringBuiltinPlan {
    pub(crate) fn span(&self) -> usize {
        match &self.recipe {
            StringBuiltinRecipe::Case { .. } => CASE_LEN,
            StringBuiltinRecipe::Search { .. } => SEARCH_LEN,
        }
    }

    pub(crate) const fn route(&self) -> [&'static str; 2] {
        match &self.recipe {
            StringBuiltinRecipe::Case { .. } => ["builtins", "string_case"],
            StringBuiltinRecipe::Search { .. } => ["strings", "search"],
        }
    }

    pub(crate) const fn trace_name(&self) -> &'static str {
        match &self.recipe {
            StringBuiltinRecipe::Case { .. } => "builtins_string_case_region",
            StringBuiltinRecipe::Search { .. } => "strings_search_region",
        }
    }

    pub(crate) fn execute(
        &self,
        environment: &crate::environment::Environment,
    ) -> Result<Option<Value>, crate::execute::VmError> {
        match &self.recipe {
            StringBuiltinRecipe::Case { receiver_slot } => {
                execute_case(environment, *receiver_slot)
            }
            StringBuiltinRecipe::Search {
                receiver_slot,
                needles,
            } => execute_search(environment, *receiver_slot, needles),
        }
    }
}

fn execute_case(
    environment: &crate::environment::Environment,
    receiver_slot: u16,
) -> Result<Option<Value>, crate::execute::VmError> {
    let Some(text) = own_text(environment, receiver_slot) else {
        return Ok(None);
    };
    let upper = crate::strings::to_upper_case(Some(&text))?;
    Ok(string_length(&upper).map(|length| Value::Number(length as f64)))
}

fn execute_search(
    environment: &crate::environment::Environment,
    receiver_slot: u16,
    needles: &[Value; 3],
) -> Result<Option<Value>, crate::execute::VmError> {
    let Some(text) = own_text(environment, receiver_slot) else {
        return Ok(None);
    };
    let first = string_call(crate::ops::Builtin::StringIndexOf, &text, &needles[0])?;
    let missing = string_call(crate::ops::Builtin::StringIndexOf, &text, &needles[1])?;
    let last = string_call(crate::ops::Builtin::StringLastIndexOf, &text, &needles[2])?;
    Ok(Some(Value::Array(std::rc::Rc::new(
        crate::value::ArrayData::new(vec![first, missing, last]),
    ))))
}

fn string_call(
    builtin: crate::ops::Builtin,
    receiver: &Value,
    argument: &Value,
) -> Result<Value, crate::execute::VmError> {
    crate::strings::execute_builtin(builtin, Some(receiver), std::slice::from_ref(argument))
        .expect("declared string builtin must have canonical execution")
}

fn own_text(environment: &crate::environment::Environment, slot: u16) -> Option<Value> {
    environment.with_proven_object(slot, |object| {
        let bits = crate::vm::proven_own_word(object, "text")?.plain_tagged_bits()?;
        crate::register_file::own_tagged_bits(bits)
    })?
}

fn string_length(value: &Value) -> Option<usize> {
    match value {
        Value::String(text) => Some(crate::strings::utf16_len(text)),
        Value::StringUnits(units) => Some(units.len()),
        _ => None,
    }
}

pub(crate) fn select_string_builtin(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<StringBuiltinPlan> {
    select_case(code, entries, cfg, start).or_else(|| select_search(code, entries, cfg, start))
}

fn select_case(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<StringBuiltinPlan> {
    let end = start.checked_add(CASE_LEN)?;
    let [load, text, upper, length, ret] = entries.get(start..end)? else {
        return None;
    };
    case_shape(code, start, [load, text, upper, length, ret])?;
    cfg.region_control(start, end)?;
    Some(StringBuiltinPlan {
        recipe: StringBuiltinRecipe::Case {
            receiver_slot: load.instruction.b,
        },
    })
}

fn case_shape(code: CodeView<'_>, start: usize, entries: [&BaselineEntry; CASE_LEN]) -> Option<()> {
    let [load, text, upper, length, ret] = entries.map(|entry| entry.instruction);
    let opcodes = [
        load.opcode,
        text.opcode,
        upper.opcode,
        length.opcode,
        ret.opcode,
    ];
    let expected = [
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::CallN,
        Opcode::GetN,
        Opcode::Return,
    ];
    let wiring = text.b == load.a
        && upper.b == text.a
        && upper.flags == 0
        && length.b == upper.a
        && ret.a == length.a;
    let names = name(code, start + 1) == Some("text")
        && name(code, start + 2) == Some("toUpperCase")
        && name(code, start + 3) == Some("length");
    (opcodes == expected && wiring && names).then_some(())
}

fn select_search(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<StringBuiltinPlan> {
    let end = start.checked_add(SEARCH_LEN)?;
    let slice = entries.get(start..end)?;
    search_shape(code, start, slice)?;
    cfg.region_control(start, end)?;
    let needles = [3, 8, 13]
        .map(|offset| {
            code.constant_at(start + offset)
                .map(|(_, value)| value.into())
        })
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    Some(StringBuiltinPlan {
        recipe: StringBuiltinRecipe::Search {
            receiver_slot: slice[0].instruction.b,
            needles,
        },
    })
}

fn search_shape(code: CodeView<'_>, start: usize, entries: &[BaselineEntry]) -> Option<()> {
    let expected = [
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::GetN,
        Opcode::LoadConst,
        Opcode::CallN,
    ];
    for (group, method) in ["indexOf", "indexOf", "lastIndexOf"]
        .into_iter()
        .enumerate()
    {
        let pc = group * 5;
        let instructions = &entries[pc..pc + 5];
        instructions
            .iter()
            .map(|entry| entry.instruction.opcode)
            .eq(expected)
            .then_some(())?;
        search_call_shape(code, start + pc, instructions, method)?;
    }
    let crate::ops::Op::MakeArray { dst, elements } = code.cold_at(start + 15)? else {
        return None;
    };
    let results = [
        entries[4].instruction.a,
        entries[9].instruction.a,
        entries[14].instruction.a,
    ];
    (elements.as_slice() == results
        && entries[16].instruction.opcode == Opcode::Return
        && entries[16].instruction.a == *dst)
        .then_some(())
}

fn search_call_shape(
    code: CodeView<'_>,
    pc: usize,
    entries: &[BaselineEntry],
    method: &str,
) -> Option<()> {
    let [load, text, callee, argument, call] = entries else {
        return None;
    };
    let [load, text, callee, argument, call] =
        [load, text, callee, argument, call].map(|e| e.instruction);
    let wiring = text.b == load.a
        && callee.b == text.a
        && call.b == text.a
        && call.c == callee.a
        && call.flags == 1
        && code.operand_window_at(pc + 4) == Some([argument.a].as_slice());
    (wiring && name(code, pc + 1) == Some("text") && name(code, pc + 2) == Some(method))
        .then_some(())
}

fn name(code: CodeView<'_>, pc: usize) -> Option<&str> {
    code.metadata_at(pc)?.name.as_deref()
}
