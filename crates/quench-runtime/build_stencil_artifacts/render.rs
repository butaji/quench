fn render_artifact(
    declaration: &RegionDeclaration,
    target: &str,
    compiler: &str,
    fingerprint: &str,
    extracted: &ExtractedObject,
) -> (String, String) {
    let name = declaration.name;
    let code = extracted
        .bytes
        .iter()
        .map(|byte| format!("0x{byte:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    let identifier = name.to_ascii_uppercase();
    let constant = format!(
        "const BYTES_{identifier}: &[u8] = &[{code}];{}",
        extracted
            .fallthrough
            .as_ref()
            .map_or_else(String::new, |tail| {
                let code = tail
                    .iter()
                    .map(|byte| format!("0x{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("\nconst FALLTHROUGH_{identifier}: &[u8] = &[{code}];")
            })
    );
    let entries = declaration
        .external_entries
        .iter()
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let row = format!(
        "    BuildStencilArtifact {{ name: {name:?}, artifact_id: {artifact_id:?}, key: CANONICAL_{identifier}_KEY, target: {target:?}, compiler: {compiler:?}, fingerprint: {fingerprint:?}, abi: {}, continuation_abi: {}, entry: {}, external_entries: &[{}], has_fallthrough: {}, executable: true, template_calls_helper: {}, bytes: BYTES_{identifier}, data: &[], relocations: {}, links: {}, stencil: crate::stencil_fact::Stencil {{ bytes: BYTES_{identifier}, holes: {} }}, fallthrough: {} }},",
        super::abi_expr(declaration),
        super::continuation_abi_expr(declaration),
        declaration.entry,
        entries,
        extracted.fallthrough.is_some(),
        super::target_template_calls_helper(declaration),
        relocation_expr(extracted),
        links_expr(declaration, extracted),
        holes_expr(extracted),
        extracted.fallthrough.as_ref().map_or("None".to_owned(), |_| {
            format!("Some(crate::stencil_fact::Stencil {{ bytes: FALLTHROUGH_{identifier}, holes: &[] }})")
        }),
        artifact_id = format!("{name}@{fingerprint}"),
    );
    (constant, row)
}

fn links_expr(declaration: &RegionDeclaration, extracted: &ExtractedObject) -> String {
    let successors = super::rust_assembly_recipe(declaration)
        .map(RustAssemblyRecipe::successors)
        .unwrap_or_default();
    if successors.is_empty() {
        return "&[]".to_owned();
    }
    let entries = extracted
        .relocations
        .iter()
        .map(|relocation| {
            render_link(relocation, successors)
                .unwrap_or_else(|| panic!("undeclared successor target {}", relocation.target))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("&[{entries}]")
}

fn render_link(
    relocation: &DeclaredRelocation,
    successors: &[AssemblySuccessor],
) -> Option<String> {
    let successor = successors
        .iter()
        .find(|successor| successor.target == relocation.target)?;
    let role = match successor.role {
        AssemblySuccessorRole::Next => "Next",
        AssemblySuccessorRole::True => "True",
        AssemblySuccessorRole::False => "False",
    };
    Some(format!(
        "crate::stencil_select::PhysicalLink {{ offset: {}, kind: crate::stencil_fact::HoleKind::{}, target: {:?}, role: crate::stencil_select::SuccessorRole::{role} }}",
        relocation.offset, relocation.kind, relocation.target
    ))
}

fn relocation_expr(extracted: &ExtractedObject) -> String {
    let entries = extracted
        .relocations
        .iter()
        .map(|relocation| {
            format!(
                "crate::stencil_select::PhysicalRelocation {{ offset: {}, kind: crate::stencil_fact::HoleKind::{}, target: {:?}, addend: {} }}",
                relocation.offset, relocation.kind, relocation.target, relocation.addend
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("&[{entries}]")
}

fn holes_expr(extracted: &ExtractedObject) -> String {
    let values = extracted
        .holes
        .iter()
        .map(|hole| {
            let offset = hole.offset;
            let kind = hole.kind;
            format!(
                "crate::stencil_fact::Hole {{ offset: {offset}, kind: crate::stencil_fact::HoleKind::{kind} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("&[{values}]")
}

fn fingerprint(
    target: &str,
    compiler: &str,
    flags: &[&str],
    declarations: &[RegionDeclaration],
) -> String {
    let version = command_output(Command::new(compiler).arg("-vV"), "read rustc identity");
    let features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    let rustflags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let schema = declarations
        .iter()
        .map(|item| {
            let source = if target.starts_with("aarch64") {
                super::rust_assembly_recipe(item)
                    .map(super::build_stencil_templates::assembly_source)
                    .or_else(|| {
                        super::rust_leaf_recipe(item).map(|recipe| rust_source(item.name, recipe))
                    })
            } else {
                super::rust_leaf_recipe(item)
                    .filter(|recipe| {
                        recipe.composition() == RecipeComposition::Whole
                    })
                    .map(|recipe| rust_source(item.name, recipe))
            }
            .unwrap_or_default();
            let bindings = super::rust_assembly_recipe(item)
                .map(|recipe| format!("{:?}", recipe.bindings()))
                .unwrap_or_default();
            let outputs = super::rust_assembly_recipe(item)
                .map(|recipe| format!("{:?}", recipe.outputs()))
                .unwrap_or_default();
            let continuation_abi = super::rust_assembly_recipe(item)
                .map(|recipe| format!("{:?}", recipe.internal_abi()))
                .unwrap_or_default();
            let successors = super::rust_assembly_recipe(item)
                .map(|recipe| format!("{:?}", recipe.successors()))
                .unwrap_or_default();
            format!(
                "{name}:{abi:?}:{ops:?}:{x86:?}:{arm:?}:{portable:?}:{holes:?}:{arm_holes:?}:{entry}:{external:?}:{bindings}:{outputs}:{continuation_abi}:{successors}:{source}",
                name = item.name,
                abi = item.abi,
                ops = item.operations,
                x86 = item.x86_bytes,
                arm = item.aarch64_bytes,
                portable = item.portable_bytes,
                holes = item.holes,
                arm_holes = item.aarch64_holes,
                entry = item.entry,
                external = item.external_entries,
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!(
        "{target}\n{version}\n{features}\n{rustflags}\n{flags:?}\n{schema}\nphysical-abi-v3\nobject-extractor-v2"
    )
    .bytes()
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv64-{hash:016x}")
}

fn artifact_fingerprint(
    declaration: &RegionDeclaration,
    target: &str,
    build_fingerprint: &str,
    extracted: &ExtractedObject,
) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    hash_bytes(&mut hash, build_fingerprint.as_bytes());
    hash_bytes(&mut hash, target.as_bytes());
    hash_bytes(&mut hash, format!("{:?}", declaration.abi).as_bytes());
    if let Some(recipe) = super::rust_assembly_recipe(declaration) {
        hash_bytes(&mut hash, format!("{:?}", recipe.bindings()).as_bytes());
        hash_bytes(&mut hash, format!("{:?}", recipe.outputs()).as_bytes());
        hash_bytes(&mut hash, format!("{:?}", recipe.internal_abi()).as_bytes());
        hash_bytes(&mut hash, format!("{:?}", recipe.successors()).as_bytes());
    }
    hash_bytes(&mut hash, &declaration.entry.to_le_bytes());
    for entry in declaration.external_entries {
        hash_bytes(&mut hash, &entry.to_le_bytes());
    }
    hash_bytes(
        &mut hash,
        &[u8::from(super::target_template_calls_helper(declaration))],
    );
    hash_bytes(&mut hash, &extracted.bytes);
    if let Some(fallthrough) = &extracted.fallthrough {
        hash_bytes(&mut hash, fallthrough);
    }
    for relocation in &extracted.relocations {
        hash_bytes(&mut hash, &relocation.offset.to_le_bytes());
        hash_bytes(&mut hash, relocation.kind.as_bytes());
        hash_bytes(&mut hash, relocation.target.as_bytes());
        hash_bytes(&mut hash, &relocation.addend.to_le_bytes());
    }
    for hole in &extracted.holes {
        hash_bytes(&mut hash, &hole.offset.to_le_bytes());
        hash_bytes(&mut hash, hole.kind.as_bytes());
    }
    format!("fnv64-{hash:016x}")
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}
