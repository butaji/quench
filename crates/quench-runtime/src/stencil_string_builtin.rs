//! Guarded portable string-builtin regions over canonical residual operations.
//!
//! Admission proves only static wiring. Execution first snapshots an owned
//! primitive string from a plain own data property, then calls the canonical
//! string implementation. No object borrow survives the allocating helper.

use crate::ir::Opcode;
use crate::machine::{BaselineEntry, CodeView};
use crate::value::Value;

const STRING_CASE_REGION_LEN: usize = 5;

pub(crate) struct StringCasePlan {
    receiver_slot: u16,
}

impl StringCasePlan {
    pub(crate) const fn span(&self) -> usize {
        STRING_CASE_REGION_LEN
    }

    pub(crate) fn execute(
        &self,
        environment: &crate::environment::Environment,
    ) -> Result<Option<Value>, crate::execute::VmError> {
        let Some(text) = own_text(environment, self.receiver_slot) else {
            return Ok(None);
        };
        let upper = crate::strings::to_upper_case(Some(&text))?;
        Ok(string_length(&upper).map(|length| Value::Number(length as f64)))
    }
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

pub(crate) fn select_string_case(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<StringCasePlan> {
    let end = start.checked_add(STRING_CASE_REGION_LEN)?;
    let [load, text, upper, length, ret] = entries.get(start..end)? else {
        return None;
    };
    let shape = string_case_shape(code, start, [load, text, upper, length, ret]);
    let control = cfg.region_control(start, end);
    shape?;
    control?;
    Some(StringCasePlan {
        receiver_slot: load.instruction.b,
    })
}

fn string_case_shape(
    code: CodeView<'_>,
    start: usize,
    entries: [&BaselineEntry; STRING_CASE_REGION_LEN],
) -> Option<()> {
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
    let names = metadata_name(code, start + 1) == Some("text")
        && metadata_name(code, start + 2) == Some("toUpperCase")
        && metadata_name(code, start + 3) == Some("length");
    (opcodes == expected && wiring && names).then_some(())
}

fn metadata_name(code: CodeView<'_>, pc: usize) -> Option<&str> {
    code.metadata_at(pc)?.name.as_deref()
}
