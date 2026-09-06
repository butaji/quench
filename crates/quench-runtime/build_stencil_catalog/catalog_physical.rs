// Runtime rendering for declared physical operand and output contracts.

fn render_physical_outputs(declaration: &RegionDeclaration) -> String {
    let Some(recipe) = rust_assembly_recipe(declaration) else {
        return "&[]".to_owned();
    };
    let outputs = recipe
        .outputs()
        .iter()
        .map(render_output)
        .collect::<Vec<_>>();
    format!("&[{}]", outputs.join(", "))
}

fn render_output(output: &PhysicalOutput) -> String {
    let value = match output.value {
        PhysicalOutputValue::Array => "Array",
        PhysicalOutputValue::Element => "Element",
        PhysicalOutputValue::Index => "Index",
        PhysicalOutputValue::Result => "Result",
    };
    let destination = match output.destination {
        PhysicalOutputDestination::Register(operand) => {
            format!("Register({})", render_operand(operand))
        }
        PhysicalOutputDestination::LocalSlot(operand) => {
            format!("LocalSlot({})", render_operand(operand))
        }
    };
    format!(
        "crate::stencil_select::PhysicalOutput {{ value: crate::stencil_select::PhysicalOutputValue::{value}, destination: crate::stencil_select::PhysicalOutputDestination::{destination} }}"
    )
}

fn render_physical_bindings(declaration: &RegionDeclaration) -> String {
    let Some(recipe) = rust_assembly_recipe(declaration) else {
        return "&[]".to_owned();
    };
    let bindings = recipe
        .bindings()
        .iter()
        .map(render_binding)
        .collect::<Vec<_>>();
    format!("&[{}]", bindings.join(", "))
}

fn render_binding(binding: &PhysicalBinding) -> String {
    match binding {
        PhysicalBinding::Equal(left, right) => format!(
            "crate::stencil_select::PhysicalBinding::Equal({}, {})",
            render_binding_value(*left),
            render_binding_value(*right)
        ),
        PhysicalBinding::AllDistinct(operands) => format!(
            "crate::stencil_select::PhysicalBinding::AllDistinct(&[{}])",
            operands
                .iter()
                .map(|operand| render_operand(*operand))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn render_binding_value(value: PhysicalBindingValue) -> String {
    match value {
        PhysicalBindingValue::Operand(operand) => format!(
            "crate::stencil_select::PhysicalBindingValue::Operand({})",
            render_operand(operand)
        ),
        PhysicalBindingValue::RegionStart => {
            "crate::stencil_select::PhysicalBindingValue::RegionStart".to_owned()
        }
        PhysicalBindingValue::RegionEnd => {
            "crate::stencil_select::PhysicalBindingValue::RegionEnd".to_owned()
        }
    }
}

fn render_operand(operand: PhysicalOperand) -> String {
    let field = match operand.field {
        PhysicalOperandField::A => "A",
        PhysicalOperandField::B => "B",
        PhysicalOperandField::C => "C",
    };
    format!(
        "crate::stencil_select::PhysicalOperand {{ operation: {}, field: crate::stencil_select::PhysicalOperandField::{field} }}",
        operand.operation
    )
}
