// Mechanical key, opcode, lookup and accessor rendering.

fn render_lookup_arms(declarations: &[RegionDeclaration]) -> String {
    declarations
        .iter()
        .enumerate()
        .map(|(index, declaration)| {
            format!(
                "        CANONICAL_{}_KEY => Some({index}),",
                region_key_name(declaration.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_accessors(declarations: &[RegionDeclaration]) -> String {
    declarations
        .iter()
        .map(|declaration| {
            let accessor = accessor_name(declaration.name);
            let key = region_key_name(declaration.name);
            format!(
                "pub const fn {accessor}_region_id() -> crate::stencil_fact::RegionId {{ CANONICAL_{key}_ID }}\npub const fn {accessor}_region_key() -> crate::stencil_fact::RegionKey {{ CANONICAL_{key}_KEY }}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_operations(declarations: &[RegionDeclaration]) -> String {
    declarations
        .iter()
        .map(|declaration| {
            let name = region_key_name(declaration.name);
            format!(
                "const CANONICAL_{name}_OPS: &[crate::ir::Opcode] = &[{}];",
                opcode_expr(declaration.operations)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_keys(declarations: &[RegionDeclaration]) -> String {
    declarations
        .iter()
        .map(|declaration| {
            let name = region_key_name(declaration.name);
            let id = stable_region_id(declaration.name);
            format!(
                "const CANONICAL_{name}_ID: crate::stencil_fact::RegionId = crate::stencil_fact::RegionId({id});\nconst CANONICAL_{name}_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(CANONICAL_{name}_ID, CANONICAL_{name}_OPS);"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_numeric_keys(declarations: &[RegionDeclaration]) -> String {
    render_opcode_keys(declarations, is_numeric_scalar_leaf)
}

fn render_continuation_keys(declarations: &[RegionDeclaration]) -> String {
    render_opcode_keys(declarations, is_scalar_continuation)
}

fn render_opcode_keys(
    declarations: &[RegionDeclaration],
    include: fn(&RegionDeclaration) -> bool,
) -> String {
    declarations
        .iter()
        .filter(|declaration| include(declaration))
        .map(|declaration| {
            format!(
                "    (crate::ir::Opcode::{}, CANONICAL_{}_KEY),",
                declaration.operations[0],
                region_key_name(declaration.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_numeric_scalar_leaf(declaration: &RegionDeclaration) -> bool {
    declaration.abi == DeclAbi::ScalarF64Binary
        && declaration.operations.last() == Some(&"Return")
        && rust_assembly_recipe(declaration)
            .is_none_or(|recipe| recipe.composition() == RecipeComposition::Whole)
        && declaration
            .operations
            .first()
            .is_some_and(|opcode| matches!(*opcode, "Add" | "Sub" | "Mul" | "Div" | "AddConst"))
}

fn is_scalar_continuation(declaration: &RegionDeclaration) -> bool {
    declaration.abi == DeclAbi::ScalarF64Binary
        && rust_assembly_recipe(declaration)
            .is_some_and(|recipe| recipe.composition() == RecipeComposition::LinkedFragments)
}

fn accessor_name(name: &str) -> String {
    match name {
        "set_named" => "set_n".to_owned(),
        other => other.to_owned(),
    }
}
