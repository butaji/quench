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
        equal, operand, value, AssemblyContinuation, AssemblyControlLink, AssemblyPatchHole,
        AssemblySuccessor, AssemblySuccessorRole, DeclAbi, DeclContinuationAbi, PhysicalBinding,
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
        region_key_name, AssemblySuccessor, AssemblySuccessorRole, DeclAbi, RecipeComposition,
        RegionDeclaration,
    };
    use super::leaf_catalog::{rust_leaf_recipe, RustLeafRecipe};

    fn abi_expr(_: &RegionDeclaration) -> String {
        "crate::stencil_select::RegionAbi::ScalarF64Binary".to_owned()
    }

    fn continuation_abi_expr(_: &RegionDeclaration) -> String {
        "crate::stencil_select::ContinuationAbi::None".to_owned()
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

#[test]
fn boolean_control_recipe_owns_both_successor_contracts() {
    use assembly_catalog::{rust_assembly_declaration, rust_assembly_recipe, RustAssemblyRecipe};
    use build_stencil_contract::{AssemblySuccessorRole, DeclContinuationAbi, RecipeComposition};

    let declaration = rust_assembly_declaration(RustAssemblyRecipe::BoolBranch);
    let recipe = rust_assembly_recipe(declaration).expect("boolean control recipe");
    assert_eq!(
        declaration.abi,
        build_stencil_contract::DeclAbi::ScalarWordBool
    );
    assert_eq!(recipe.composition(), RecipeComposition::ControlFragment);
    assert_eq!(recipe.internal_abi(), DeclContinuationAbi::WordX0);
    assert_eq!(recipe.successors(), &[]);
    assert_eq!(
        recipe
            .control_links()
            .iter()
            .map(|link| (link.offset, link.role, link.target))
            .collect::<Vec<_>>(),
        vec![
            (4, AssemblySuccessorRole::False, "q_bool_branch_false"),
            (8, AssemblySuccessorRole::True, "q_bool_branch_true"),
        ]
    );

    let terminal = rust_assembly_declaration(RustAssemblyRecipe::ReturnWord);
    assert_eq!(terminal.operations, ["Return"]);
    assert_eq!(terminal.abi, declaration.abi);
    let terminal_recipe = rust_assembly_recipe(terminal).expect("return fragment recipe");
    assert_eq!(terminal_recipe.internal_abi(), DeclContinuationAbi::WordX0);
    assert_eq!(terminal_recipe.composition(), RecipeComposition::Whole);

    let constant = rust_assembly_declaration(RustAssemblyRecipe::WordConstFragment);
    let constant_recipe = rust_assembly_recipe(constant).expect("word constant recipe");
    assert_eq!(constant.operations, ["LoadConst"]);
    assert_eq!(constant_recipe.internal_abi(), DeclContinuationAbi::WordX0);
    assert_eq!(constant_recipe.patch_holes().len(), 1);
    assert_eq!(constant_recipe.patch_holes()[0].kind, "Literal64");
    assert_eq!(constant_recipe.control_links().len(), 1);
    assert_eq!(
        constant_recipe.control_links()[0].role,
        AssemblySuccessorRole::Next
    );
}
