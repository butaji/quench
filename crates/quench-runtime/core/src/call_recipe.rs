use crate::dynbytecode::{ARGUMENTS_BINDING_NAME, CatchBinding, DynCode, DynOp, THIS_BINDING_NAME};
use std::collections::HashMap;
use std::rc::Rc;

/// Immutable, owned facts shared by every lowering of one function call.
///
/// The runtime ABI record, static analysis, and future inline plans must be
/// projections of this value. They must not rediscover the same facts from
/// bytecode independently.
pub(crate) struct CallBindingRecipe {
    binding_layout: Rc<HashMap<String, usize>>,
    parameter_slots: Box<[usize]>,
    this_slot: usize,
    arguments_slot: usize,
    uses_arguments: bool,
    captures_frame: bool,
}

impl CallBindingRecipe {
    pub(crate) fn from_code(code: &DynCode) -> Self {
        let binding_layout = Rc::new(binding_layout(code));
        let this_slot = binding_layout[THIS_BINDING_NAME];
        let arguments_slot = binding_layout[ARGUMENTS_BINDING_NAME];
        let parameter_slots = code
            .params
            .iter()
            .map(|parameter| binding_layout[parameter])
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            uses_arguments: uses_arguments(code, arguments_slot),
            captures_frame: captures_frame(code),
            binding_layout,
            parameter_slots,
            this_slot,
            arguments_slot,
        }
    }

    pub(crate) fn binding_layout(&self) -> &Rc<HashMap<String, usize>> {
        &self.binding_layout
    }

    pub(crate) fn local_count(&self) -> usize {
        self.binding_layout.len()
    }

    pub(crate) fn parameter_slots(&self) -> &[usize] {
        &self.parameter_slots
    }

    pub(crate) fn this_slot(&self) -> usize {
        self.this_slot
    }

    pub(crate) fn arguments_slot(&self) -> usize {
        self.arguments_slot
    }

    pub(crate) fn uses_arguments(&self) -> bool {
        self.uses_arguments
    }

    pub(crate) fn captures_frame(&self) -> bool {
        self.captures_frame
    }

    pub(crate) fn into_storage(self) -> (Rc<HashMap<String, usize>>, Box<[usize]>) {
        (self.binding_layout, self.parameter_slots)
    }
}

#[cfg(feature = "inline-census")]
pub(crate) fn initial_inline_facts(
    code: &DynCode,
    captures_frame: bool,
    uses_arguments_object: bool,
    parameter_count: usize,
    argument_count: usize,
    code_bytes: usize,
    frame_slots: usize,
) -> crate::inline_plan::InitialInlineFacts {
    crate::inline_plan::InitialInlineFacts {
        captures_frame,
        uses_arguments_object,
        exact_arity: parameter_count == argument_count,
        has_nested_call: has_nested_call(code),
        is_straight_line: is_straight_line(code),
        code_bytes,
        frame_slots,
    }
}

#[cfg(feature = "inline-census")]
fn has_nested_call(code: &DynCode) -> bool {
    code.ops.iter().any(|instruction| {
        matches!(
            &instruction.op,
            DynOp::Call { .. } | DynOp::Construct { .. }
        )
    })
}

#[cfg(feature = "inline-census")]
fn is_straight_line(code: &DynCode) -> bool {
    let final_pc = code.ops.len().checked_sub(1);
    code.ops
        .iter()
        .enumerate()
        .all(|(pc, instruction)| match &instruction.op {
            DynOp::Return { .. } => Some(pc) == final_pc,
            DynOp::Jump { .. }
            | DynOp::JumpIfFalse { .. }
            | DynOp::ForInInit { .. }
            | DynOp::ForInNext { .. }
            | DynOp::PushHandler { .. }
            | DynOp::PopHandler
            | DynOp::Catch { .. }
            | DynOp::Throw { .. }
            | DynOp::Rethrow => false,
            _ => true,
        })
}

fn binding_layout(code: &DynCode) -> HashMap<String, usize> {
    if !code.bindings.is_empty() {
        return code
            .bindings
            .iter()
            .enumerate()
            .map(|(slot, name)| (name.clone(), slot))
            .collect();
    }

    reconstructed_binding_layout(code)
}

fn reconstructed_binding_layout(code: &DynCode) -> HashMap<String, usize> {
    let mut layout = HashMap::new();
    let mut add_binding = |name: &str| {
        let next_slot = layout.len();
        layout.entry(name.to_owned()).or_insert(next_slot);
    };
    add_binding(THIS_BINDING_NAME);
    add_binding(ARGUMENTS_BINDING_NAME);
    for parameter in &code.params {
        add_binding(parameter);
    }
    for (name, _) in &code.hoisted {
        add_binding(name);
    }
    for instruction in &code.ops {
        match &instruction.op {
            DynOp::DeclareName { name, .. } => add_binding(name),
            DynOp::Catch {
                binding: Some(CatchBinding::Name(name)),
            } => add_binding(name),
            _ => {}
        }
    }
    layout
}

fn uses_arguments(code: &DynCode, arguments_slot: usize) -> bool {
    code.ops.iter().any(|instruction| {
        matches!(
            &instruction.op,
            DynOp::LoadName { name, .. } | DynOp::StoreName { name, .. }
                if name == ARGUMENTS_BINDING_NAME
        ) || matches!(
            &instruction.op,
            DynOp::LoadLocal { slot, .. } | DynOp::StoreLocal { slot, .. }
                if *slot == arguments_slot
        )
    })
}

fn captures_frame(code: &DynCode) -> bool {
    !code.hoisted.is_empty()
        || code
            .ops
            .iter()
            .any(|instruction| matches!(instruction.op, DynOp::MakeClosure { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynbytecode::{DynInstr, Literal};
    use oxc_span::Span;

    const RESULT_REGISTER: u16 = 0;
    const LOCAL_BINDING_NAME: &str = "result";

    fn code_with(ops: Vec<DynOp>) -> DynCode {
        DynCode {
            ops: ops
                .into_iter()
                .map(|op| DynInstr {
                    op,
                    span: Span::default(),
                })
                .collect(),
            registers: usize::from(RESULT_REGISTER) + 1,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: Vec::new(),
            bindings: vec![
                THIS_BINDING_NAME.into(),
                ARGUMENTS_BINDING_NAME.into(),
                LOCAL_BINDING_NAME.into(),
            ],
            is_script: false,
        }
    }

    #[test]
    fn recipe_owns_one_canonical_binding_view() {
        let code = code_with(vec![DynOp::LoadLiteral {
            dst: RESULT_REGISTER,
            value: Literal::Undefined,
        }]);
        let recipe = CallBindingRecipe::from_code(&code);

        assert_eq!(recipe.local_count(), code.bindings.len());
        assert_eq!(recipe.this_slot(), 0);
        assert_eq!(recipe.arguments_slot(), 1);
        assert!(!recipe.uses_arguments());
        assert!(!recipe.captures_frame());
    }

    #[test]
    fn arguments_use_is_derived_once_from_lowered_local_access() {
        let code = code_with(vec![DynOp::LoadLocal {
            dst: RESULT_REGISTER,
            slot: 1,
        }]);
        assert!(CallBindingRecipe::from_code(&code).uses_arguments());
    }
}
