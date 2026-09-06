#[path = "../build_stencil_contract.rs"]
mod build_stencil_contract;

mod harness {
    use super::build_stencil_contract::{
        DeclAbi, RegionDeclaration, RustLeafRecipe, rust_leaf_recipe,
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
