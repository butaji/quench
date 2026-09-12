// Mechanical key, opcode, lookup and accessor rendering.

macro_rules! binary_region_hints {
    ($($operator:ident = $id:literal => $region:expr),+ $(,)?) => {
        const BINARY_REGION_HINTS: &[(&str, Option<&str>)] = &[
            $( (stringify!($operator), $region), )+
        ];
    };
}

include!("../operator_catalog.rs");
with_binary_operator_catalog!(binary_region_hints);

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
    let mut rows = declarations
        .iter()
        .filter(|declaration| {
            is_numeric_scalar_leaf(declaration)
                && declaration.operations.first() != Some(&"IncI")
        })
        .map(|declaration| {
            format!(
                "    (crate::ir::Opcode::{}, CANONICAL_{}_KEY),",
                declaration.operations[0],
                region_key_name(declaration.name)
            )
        })
        .collect::<Vec<_>>();
    // Numeric update opcodes are canonical ±1 forms, not binary arithmetic.
    // Their physical families are declared in the shared operator catalog and
    // derived here so the generated key table remains the sole selector fact.
    for &(operator, region) in BINARY_REGION_HINTS {
        let Some(region) = region else { continue };
        let Some(declaration) = declarations.iter().find(|declaration| {
            declaration.name == region && is_numeric_scalar_leaf(declaration)
        }) else {
            continue;
        };
        if declaration
            .operations
            .first()
            .is_some_and(|opcode| **opcode == *operator)
        {
            continue;
        }
        rows.push(format!(
            "    (crate::ir::Opcode::{operator}, CANONICAL_{}_KEY),",
            region_key_name(declaration.name)
        ));
    }
    rows.join("\n")
}

/// Render the one generated mapping used when a generic `Binary` instruction
/// needs a physical comparison/bitwise region. Dedicated numeric opcodes are
/// resolved through `numeric_region_key`; only operators whose physical leaf
/// has a distinct region declaration appear here.
fn render_binary_keys(declarations: &[RegionDeclaration]) -> String {
    let mut arms = Vec::new();
    let mut branch_arms = Vec::new();
    for &(operator, region) in BINARY_REGION_HINTS {
        let Some(name) = region else { continue };
        let declaration = declarations
            .iter()
            .find(|declaration| declaration.name == name)
            .unwrap_or_else(|| panic!("binary operator mapping requires region {name}"));
        let key = region_key_name(declaration.name);
        arms.push(format!(
            "        crate::ops::BinaryOp::{operator} => Some(CANONICAL_{key}_KEY),"
        ));
        if name.starts_with("compare_") {
            let branch_name = format!("{name}_branch");
            let branch = declarations
                .iter()
                .find(|declaration| declaration.name == branch_name)
                .unwrap_or_else(|| panic!("binary branch mapping requires region {branch_name}"));
            let branch_key = region_key_name(branch.name);
            branch_arms.push(format!(
                "        crate::ops::BinaryOp::{operator} => Some(CANONICAL_{branch_key}_KEY),"
            ));
        }
    }
    format!(
        "pub(crate) fn binary_region_key(operator: crate::ops::BinaryOp) -> Option<crate::stencil_fact::RegionKey> {{\n    match operator {{\n{}\n        _ => None,\n    }}\n}}\n\npub(crate) fn binary_branch_region_key(operator: crate::ops::BinaryOp) -> Option<crate::stencil_fact::RegionKey> {{\n    match operator {{\n{}\n        _ => None,\n    }}\n}}",
        arms.join("\n"),
        branch_arms.join("\n")
    )
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
            .is_some_and(|opcode| {
                matches!(
                    *opcode,
                    "Add" | "Sub" | "Mul" | "Div" | "AddConst" | "IncI"
                )
            })
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
