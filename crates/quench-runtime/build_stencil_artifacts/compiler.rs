fn rust_source(name: &str, recipe: super::RustLeafRecipe) -> String {
    assert_valid_symbol_name(name);
    format!(
        "#![no_std]\n#[no_mangle]\n#[inline(never)]\npub extern \"C\" fn q_{}({}) -> {} {{ {} }}\n",
        name,
        recipe.parameters(),
        recipe.result(),
        recipe.expression()
    )
}

fn assert_valid_symbol_name(name: &str) {
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
    assert!(valid_start, "invalid Rust stencil symbol name {name:?}");
    assert!(
        chars.all(|character| character == '_' || character.is_ascii_alphanumeric()),
        "invalid Rust stencil symbol name {name:?}"
    );
}

fn compile_one(
    root: &Path,
    target: &str,
    compiler: &str,
    flags: &[&str],
    rustflags: &[String],
    declaration: &RegionDeclaration,
    recipe: super::RustLeafRecipe,
) -> Vec<u8> {
    let source = root.join(format!("{}.rs", declaration.name));
    let object = root.join(format!("{}.o", declaration.name));
    fs::write(&source, rust_source(declaration.name, recipe)).expect("write Rust stencil source");
    let mut command = Command::new(compiler);
    command
        .args(["--target", target])
        .args(flags)
        .args(rustflags)
        .args([source.to_str().unwrap(), "-o", object.to_str().unwrap()]);
    run(&mut command, "compile Rust stencil object");
    parse_object(&object, &format!("q_{}", declaration.name))
}

fn compile_fragment_pair(
    root: &Path,
    target: &str,
    compiler: &str,
    flags: &[&str],
    rustflags: &[String],
    declaration: &RegionDeclaration,
    recipe: RustAssemblyRecipe,
) -> ExtractedObject {
    if !target.starts_with("aarch64") {
        return ExtractedObject {
            bytes: Vec::new(),
            fallthrough: None,
            relocations: Vec::new(),
            holes: Vec::new(),
        };
    }
    let (head_source, tail_source) = super::build_stencil_templates::fragment_sources(recipe)
        .expect("declared fragment-pair source");
    let continuation = recipe
        .continuation()
        .expect("fragment pair must declare its continuation");
    let expected = declaration
        .aarch64_holes
        .iter()
        .map(|(offset, width, kind)| ExpectedRelocation {
            section: SectionKind::Text,
            offset: u64::from(*offset),
            width: *width,
            kind,
            target: continuation.target,
            addend: 0,
        })
        .collect::<Vec<_>>();
    let head = compile_assembly_fragment(
        root,
        target,
        compiler,
        flags,
        rustflags,
        continuation.head_name,
        head_source,
        &expected,
        &[],
    );
    let tail = compile_assembly_fragment(
        root,
        target,
        compiler,
        flags,
        rustflags,
        continuation.tail_name,
        tail_source,
        &[],
        &[],
    );
    ExtractedObject {
        bytes: head.bytes,
        fallthrough: Some(tail.bytes),
        relocations: head.relocations,
        holes: head.holes,
    }
}

fn compile_assembly_fragment(
    root: &Path,
    target: &str,
    compiler: &str,
    flags: &[&str],
    rustflags: &[String],
    name: &str,
    source_text: &str,
    expected_relocations: &[ExpectedRelocation],
    expected_holes: &[ExtractedHole],
) -> ParsedFragment {
    let source = root.join(format!("{name}.rs"));
    let object = root.join(format!("{name}.o"));
    fs::write(&source, source_text).expect("write Rust assembly fragment");
    let mut command = Command::new(compiler);
    command
        .args(["--target", target])
        .args(flags)
        .args(rustflags)
        .args([source.to_str().unwrap(), "-o", object.to_str().unwrap()]);
    run(&mut command, "compile Rust assembly fragment");
    parse_object_range(
        &object,
        &format!("q_{name}"),
        &format!("q_{name}_end"),
        expected_relocations,
        expected_holes,
    )
}
