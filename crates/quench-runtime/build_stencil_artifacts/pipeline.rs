fn empty_artifacts() -> String {
    format!(
        "{HEADER}{}\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[];\nfn build_stencil_artifact_lookup(_: crate::stencil_fact::RegionKey) -> Option<&'static BuildStencilArtifact> {{ None }}\n",
        artifact_schema()
    )
}

fn artifact_schema() -> &'static str {
    "#[derive(Clone, Copy, Debug)] pub struct BuildStencilArtifact { pub name: &'static str, pub artifact_id: &'static str, pub key: crate::stencil_fact::RegionKey, pub target: &'static str, pub compiler: &'static str, pub fingerprint: &'static str, pub abi: crate::stencil_select::RegionAbi, pub continuation_abi: crate::stencil_select::ContinuationAbi, pub entry: u16, pub external_entries: &'static [u16], pub has_fallthrough: bool, pub executable: bool, pub template_calls_helper: bool, pub bytes: &'static [u8], pub data: &'static [u8], pub relocations: &'static [crate::stencil_select::PhysicalRelocation], pub stencil: crate::stencil_fact::Stencil, pub fallthrough: Option<crate::stencil_fact::Stencil> }"
}

fn extract_objects(declarations: &[RegionDeclaration]) -> String {
    let target = env::var("TARGET").expect("TARGET for stencil object generation");
    let compiler = rustc_path();
    let flags = [
        "--crate-type=lib",
        "--emit=obj",
        "-Copt-level=2",
        "-Cpanic=abort",
        "-Coverflow-checks=off",
        "--edition=2021",
    ];
    let rustflags = effective_rustflags();
    let build_fingerprint = fingerprint(&target, &compiler, &flags, declarations);
    let root = unique_directory();
    let mut constants = Vec::new();
    let mut rows = Vec::new();
    let mut lookup_arms = Vec::new();
    for declaration in declarations {
        // A generated whole-function recipe owns its arguments directly, so
        // it does not need the canonical byte-template holes (AddConst is the
        // first example). Unsupported hole-bearing recipes remain skipped
        // until a declared relocation plan exists.
        let assembly = target
            .starts_with("aarch64")
            .then(|| super::rust_assembly_recipe(declaration))
            .flatten();
        let extracted = if let Some(recipe) = assembly {
            if recipe.composition() != RecipeComposition::Whole {
                compile_fragment_pair(
                    &root.path,
                    &target,
                    &compiler,
                    &flags,
                    &rustflags,
                    declaration,
                    recipe,
                )
            } else {
                let source = super::build_stencil_templates::assembly_source(recipe);
                let expected_holes = expected_holes(declaration, &target);
                let parsed = compile_assembly_fragment(
                    &root.path,
                    &target,
                    &compiler,
                    &flags,
                    &rustflags,
                    declaration.name,
                    &source,
                    &[],
                    &expected_holes,
                );
                ExtractedObject {
                    bytes: parsed.bytes,
                    fallthrough: None,
                    relocations: parsed.relocations,
                    holes: parsed.holes,
                }
            }
        } else {
            let Some(recipe) = super::rust_leaf_recipe(declaration) else {
                continue;
            };
            if recipe.composition() != RecipeComposition::Whole {
                continue;
            }
            ExtractedObject {
                bytes: compile_one(
                    &root.path,
                    &target,
                    &compiler,
                    &flags,
                    &rustflags,
                    declaration,
                    recipe,
                ),
                fallthrough: None,
                relocations: Vec::new(),
                holes: Vec::new(),
            }
        };
        let fingerprint =
            artifact_fingerprint(declaration, &target, &build_fingerprint, &extracted);
        let (constant, row) =
            render_artifact(declaration, &target, &compiler, &fingerprint, &extracted);
        constants.push(constant);
        lookup_arms.push(format!(
            "        CANONICAL_{}_KEY => Some(&BUILD_STENCIL_ARTIFACTS[{}]),",
            region_key_name(declaration.name),
            rows.len()
        ));
        rows.push(row);
    }
    assert!(!rows.is_empty(), "no extractable Rust stencil declarations");
    let generated = format!(
        "{HEADER}{}\n{}\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[\n{}\n];\nfn build_stencil_artifact_lookup(key: crate::stencil_fact::RegionKey) -> Option<&'static BuildStencilArtifact> {{\n    match key {{\n{}\n        _ => None,\n    }}\n}}\n",
        artifact_schema(),
        constants.join("\n"),
        rows.join("\n"),
        lookup_arms.join("\n")
    );
    generated
}
