#[macro_use]
#[path = "../build_stencil_contract.rs"]
mod build_stencil_contract;

mod leaf_catalog {
    use super::build_stencil_contract::{DeclAbi, RecipeComposition, RegionDeclaration};

    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/build_stencil_catalog/encoding_common.rs"
    ));
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/build_stencil_catalog/encoding_x86.rs"
    ));
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/build_stencil_catalog/encoding_aarch64.rs"
    ));
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/build_stencil_catalog/declarations_rust_leaf.rs"
    ));
}

mod assembly_catalog {
    use super::build_stencil_contract::{
        equal, operand, value, AssemblyContinuation, DeclAbi, PhysicalBinding,
        PhysicalBindingValue, PhysicalOperand, PhysicalOperandField, PhysicalOutput,
        PhysicalOutputDestination, PhysicalOutputValue, RecipeComposition, RegionDeclaration,
    };

    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/build_stencil_catalog/encoding_common.rs"
    ));
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/build_stencil_catalog/encoding_x86.rs"
    ));
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/build_stencil_catalog/encoding_aarch64.rs"
    ));
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/build_stencil_catalog/declarations_rust_assembly.rs"
    ));
}

mod harness {
    use super::assembly_catalog::{
        rust_assembly_declaration, rust_assembly_recipe, RustAssemblyRecipe,
    };
    use super::build_stencil_contract::{
        region_key_name, DeclAbi, RecipeComposition, RegionDeclaration,
    };
    use super::leaf_catalog::{rust_leaf_recipe, RustLeafRecipe};

    fn abi_expr(_: &RegionDeclaration) -> String {
        "crate::stencil_select::RegionAbi::ScalarF64Binary".to_owned()
    }

    fn target_template_calls_helper(_: &RegionDeclaration) -> bool {
        false
    }

    mod build_stencil_templates {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/build_stencil_templates.rs"
        ));
    }

    mod implementation {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/build_stencil_artifacts.rs"
        ));
    }
}

fn declaration(
    name: &'static str,
    abi: build_stencil_contract::DeclAbi,
    operations: &'static [&'static str],
) -> build_stencil_contract::RegionDeclaration {
    build_stencil_contract::RegionDeclaration {
        name,
        operations,
        abi,
        x86_bytes: &[],
        aarch64_bytes: &[],
        portable_bytes: &[],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    }
}

#[test]
fn rust_leaf_recipe_requires_full_physical_contract() {
    use build_stencil_contract::DeclAbi;
    use leaf_catalog::{rust_leaf_declaration, rust_leaf_recipe, RustLeafRecipe};

    let exact = *rust_leaf_declaration(RustLeafRecipe::Equal);
    assert_eq!(rust_leaf_recipe(&exact), Some(RustLeafRecipe::Equal));
    let mut wrong_name = exact;
    wrong_name.name = "unknown_equal";
    let mut wrong_abi = exact;
    wrong_abi.abi = DeclAbi::ScalarF64Binary;
    let mut wrong_ops = exact;
    wrong_ops.operations = &["Add", "Return"];
    let mutations = [wrong_name, wrong_abi, wrong_ops];
    assert!(mutations
        .iter()
        .all(|item| rust_leaf_recipe(item).is_none()));

    let mut wrong_bytes = exact;
    wrong_bytes.aarch64_bytes = &[0xC0, 0x03, 0x5F, 0xD6];
    assert!(rust_leaf_recipe(&wrong_bytes).is_none());
}

#[test]
fn rust_leaf_recipe_rejects_layout_drift() {
    use leaf_catalog::{rust_leaf_declaration, rust_leaf_recipe, RustLeafRecipe};

    let exact = *rust_leaf_declaration(RustLeafRecipe::AddConst);
    let mut wrong_hole = exact;
    wrong_hole.aarch64_holes = &[];
    let mut wrong_entry = exact;
    wrong_entry.entry = 4;
    let mut wrong_external_entry = exact;
    wrong_external_entry.external_entries = &[0, 4];
    let mut wrong_portable = exact;
    wrong_portable.portable_bytes = &[];
    let mutations = [
        wrong_hole,
        wrong_entry,
        wrong_external_entry,
        wrong_portable,
    ];
    assert!(mutations
        .iter()
        .all(|item| rust_leaf_recipe(item).is_none()));
}

#[test]
fn rust_assembly_recipe_requires_name_abi_and_residual_shape() {
    use assembly_catalog::{rust_assembly_declaration, rust_assembly_recipe, RustAssemblyRecipe};
    use build_stencil_contract::{DeclAbi, RecipeComposition};

    let exact = *rust_assembly_declaration(RustAssemblyRecipe::Move);
    assert_eq!(rust_assembly_recipe(&exact), Some(RustAssemblyRecipe::Move));
    let mut wrong_abi = exact;
    wrong_abi.abi = DeclAbi::ScalarF64Binary;
    let mut wrong_ops = exact;
    wrong_ops.operations = &["LoadLocal"];
    let mut wrong_name = exact;
    wrong_name.name = "load_local";
    assert!([wrong_abi, wrong_ops, wrong_name]
        .iter()
        .all(|item| rust_assembly_recipe(item).is_none()));

    let mut missing_hole = *rust_assembly_declaration(RustAssemblyRecipe::LoadConst);
    missing_hole.aarch64_holes = &[];
    assert!(rust_assembly_recipe(&missing_hole).is_none());
    missing_hole = *rust_assembly_declaration(RustAssemblyRecipe::LoadConst);
    assert_eq!(
        rust_assembly_recipe(&missing_hole),
        Some(RustAssemblyRecipe::LoadConst)
    );

    let mut add_chain = *rust_assembly_declaration(RustAssemblyRecipe::AddChain);
    assert_eq!(
        rust_assembly_recipe(&add_chain),
        Some(RustAssemblyRecipe::AddChain)
    );
    let recipe = rust_assembly_recipe(&add_chain).expect("add-chain recipe");
    assert_eq!(recipe.composition(), RecipeComposition::LinkedFragments);
    let continuation = recipe.continuation().expect("add-chain continuation");
    assert_eq!(continuation.head_name, "add_chain_head");
    assert_eq!(continuation.tail_name, "add_chain_tail");
    assert_eq!(continuation.target, "q_add_chain_tail");
    add_chain.aarch64_holes = &[(8, 4, "Branch26")];
    assert!(rust_assembly_recipe(&add_chain).is_none());
}
