//! Canonical pure property-return facts derived from residual operations.

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct OwnFieldReturn {
    pub(crate) field: std::rc::Rc<str>,
}

pub(crate) fn stable_own_field_return(
    installed: &mut Option<OwnFieldReturn>,
    function: &crate::value::FunctionValue,
) -> Option<OwnFieldReturn> {
    let fact = own_field_return(function)?;
    let expected = installed.get_or_insert_with(|| fact.clone());
    (*expected == fact).then_some(fact)
}

fn own_field_return(function: &crate::value::FunctionValue) -> Option<OwnFieldReturn> {
    (function.params == 1 && crate::functions::direct_call_eligible(function)).then_some(())?;
    let code = function.code.code()?;
    (code.len() == 5).then_some(())?;
    let load = code.instruction(0)?;
    let get = code.instruction(1)?;
    let ret = code.instruction(2)?;
    let parameter = u16::try_from(function.captures.len()).ok()?;
    (load.opcode == crate::ir::Opcode::LoadLocal
        && load.b == parameter
        && get.opcode.semantic_opcode() == crate::ir::Opcode::GetN
        && get.b == load.a
        && ret == crate::ir::Instruction::ret(get.a)
        && undefined_tail(code))
    .then_some(OwnFieldReturn {
        field: code.metadata_at(1)?.name.clone()?,
    })
}

fn undefined_tail(code: crate::machine::CodeView<'_>) -> bool {
    let Some(load) = code.instruction(3) else {
        return false;
    };
    load.opcode == crate::ir::Opcode::LoadConst
        && matches!(code.constant(load.b), Some(crate::ops::Constant::Undefined))
        && code.instruction(4) == Some(crate::ir::Instruction::ret(load.a))
}
