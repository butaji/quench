#[path = "../build_stencil_contract.rs"]
mod build_stencil_contract;

mod harness {
    use super::build_stencil_contract::{
        rust_assembly_recipe, rust_leaf_recipe, DeclAbi, RecipeComposition, RegionDeclaration,
        RustAssemblyRecipe, RustLeafRecipe,
    };

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
fn rust_leaf_recipe_requires_name_abi_and_residual_shape() {
    use build_stencil_contract::{rust_leaf_recipe, DeclAbi, RustLeafRecipe};

    let exact = declaration("compare_equal", DeclAbi::ScalarBool, &["Binary", "Return"]);
    assert_eq!(rust_leaf_recipe(&exact), Some(RustLeafRecipe::Equal));
    assert!(rust_leaf_recipe(&declaration(
        "compare_equal",
        DeclAbi::ScalarF64Binary,
        &["Binary", "Return"]
    ))
    .is_none());
    assert!(rust_leaf_recipe(&declaration(
        "compare_equal",
        DeclAbi::ScalarBool,
        &["Add", "Return"]
    ))
    .is_none());
    assert!(rust_leaf_recipe(&declaration(
        "unknown_equal",
        DeclAbi::ScalarBool,
        &["Binary", "Return"]
    ))
    .is_none());
}

#[test]
fn rust_assembly_recipe_requires_name_abi_and_residual_shape() {
    use build_stencil_contract::{
        recipe_composition, rust_assembly_recipe, DeclAbi, RecipeComposition, RustAssemblyRecipe,
    };

    let exact = declaration("move", DeclAbi::TaggedWord, &["Move"]);
    assert_eq!(rust_assembly_recipe(&exact), Some(RustAssemblyRecipe::Move));
    assert!(
        rust_assembly_recipe(&declaration("move", DeclAbi::ScalarF64Binary, &["Move"])).is_none()
    );
    assert!(
        rust_assembly_recipe(&declaration("move", DeclAbi::TaggedWord, &["LoadLocal"])).is_none()
    );
    assert!(
        rust_assembly_recipe(&declaration("load_local", DeclAbi::TaggedWord, &["Move"])).is_none()
    );

    let mut missing_hole = declaration(
        "load_const",
        DeclAbi::ConstantWord,
        &["LoadConst", "Return"],
    );
    assert!(rust_assembly_recipe(&missing_hole).is_none());
    missing_hole.aarch64_holes = &[(8, 8, "Literal64")];
    assert_eq!(
        rust_assembly_recipe(&missing_hole),
        Some(RustAssemblyRecipe::LoadConst)
    );

    let mut add_chain = declaration("add_chain", DeclAbi::ScalarF64x3, &["Add", "Add"]);
    add_chain.aarch64_holes = &[(4, 4, "Branch26")];
    assert_eq!(
        rust_assembly_recipe(&add_chain),
        Some(RustAssemblyRecipe::AddChain)
    );
    assert_eq!(recipe_composition(&add_chain), RecipeComposition::AddChain);
    add_chain.aarch64_holes = &[(8, 4, "Branch26")];
    assert!(rust_assembly_recipe(&add_chain).is_none());
}
