#[path = "../build_stencil_contract.rs"]
mod build_stencil_contract;

mod harness {
    use super::build_stencil_contract::{
        rust_assembly_recipe, rust_leaf_recipe, DeclAbi, RegionDeclaration, RustLeafRecipe,
    };

    fn abi_expr(_: &RegionDeclaration) -> String {
        "crate::stencil_select::RegionAbi::Scalar".to_owned()
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
        DeclAbi::Scalar,
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
    use build_stencil_contract::{rust_assembly_recipe, DeclAbi, RustAssemblyRecipe};

    let exact = declaration("move", DeclAbi::TaggedWord, &["Move"]);
    assert_eq!(rust_assembly_recipe(&exact), Some(RustAssemblyRecipe::Move));
    assert!(rust_assembly_recipe(&declaration("move", DeclAbi::Scalar, &["Move"])).is_none());
    assert!(
        rust_assembly_recipe(&declaration("move", DeclAbi::TaggedWord, &["LoadLocal"])).is_none()
    );
    assert!(
        rust_assembly_recipe(&declaration("load_local", DeclAbi::TaggedWord, &["Move"])).is_none()
    );
}
