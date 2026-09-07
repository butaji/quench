// Mechanical relocation-to-successor rendering from assembly declarations.

fn render_links(declarations: &[RegionDeclaration]) -> String {
    declarations
        .iter()
        .map(render_declaration_links)
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_declaration_links(declaration: &RegionDeclaration) -> String {
    let name = region_key_name(declaration.name);
    let Some(successor) = rust_assembly_recipe(declaration)
        .filter(|recipe| recipe.successors().len() == 1)
        .and_then(|recipe| recipe.successors().first())
    else {
        return format!(
            "const CANONICAL_{name}_LINKS: &[crate::stencil_select::PhysicalLink] = &[];"
        );
    };
    let x86 = render_target_links(declaration.holes, successor);
    let aarch64 = render_target_links(declaration.aarch64_holes, successor);
    format!(
        "#[cfg(target_arch = \"x86_64\")]\nconst CANONICAL_{name}_LINKS: &[crate::stencil_select::PhysicalLink] = &[\n{x86}\n];\n#[cfg(target_arch = \"aarch64\")]\nconst CANONICAL_{name}_LINKS: &[crate::stencil_select::PhysicalLink] = &[\n{aarch64}\n];\n#[cfg(not(any(target_arch = \"x86_64\", target_arch = \"aarch64\")))]\nconst CANONICAL_{name}_LINKS: &[crate::stencil_select::PhysicalLink] = &[];"
    )
}

fn render_target_links(
    holes: &[(u16, usize, &'static str)],
    successor: &AssemblySuccessor,
) -> String {
    let target = successor.target;
    let role = match successor.role {
        AssemblySuccessorRole::Next => "Next",
        AssemblySuccessorRole::True => "True",
        AssemblySuccessorRole::False => "False",
    };
    holes
        .iter()
        .filter(|(_, _, kind)| matches!(*kind, "Rel32" | "Branch26" | "CondBranch19"))
        .map(|(offset, _, kind)| {
            format!(
                "    crate::stencil_select::PhysicalLink {{ offset: {offset}, kind: crate::stencil_fact::HoleKind::{kind}, target: {target:?}, role: crate::stencil_select::SuccessorRole::{role} }},"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
