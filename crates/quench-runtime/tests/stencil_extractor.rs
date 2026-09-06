mod harness {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum DeclAbi {
        Scalar,
    }

    #[derive(Clone, Copy, Debug)]
    struct RegionDeclaration {
        name: &'static str,
        operations: &'static [&'static str],
        abi: DeclAbi,
        x86_bytes: &'static [u8],
        aarch64_bytes: &'static [u8],
        portable_bytes: &'static [u8],
        holes: &'static [(u16, usize, &'static str)],
        aarch64_holes: &'static [(u16, usize, &'static str)],
        entry: u32,
        external_entries: &'static [u32],
    }

    #[derive(Clone, Copy)]
    enum RustLeafRecipe {
        Dummy,
    }

    impl RustLeafRecipe {
        fn expression(self) -> &'static str {
            "left + right"
        }

        fn parameters(self) -> &'static str {
            "left: f64, right: f64"
        }
    }

    fn rust_leaf_recipe(_: &RegionDeclaration) -> Option<RustLeafRecipe> {
        None
    }

    fn abi_expr(_: &RegionDeclaration) -> String {
        "crate::stencil_select::RegionAbi::Scalar".to_owned()
    }

    fn target_template_calls_helper(_: &RegionDeclaration) -> bool {
        false
    }

    mod implementation {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/build_stencil_artifacts.rs"
        ));
    }
}
