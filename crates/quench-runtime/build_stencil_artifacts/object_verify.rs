fn parse_object(path: &Path, name: &str) -> Vec<u8> {
    let data = fs::read(path).expect("read Rust stencil object");
    let file = object::File::parse(&*data).expect("parse Rust stencil object");
    assert_target_architecture(&file, env::var("TARGET").ok().as_deref());
    assert_target_format(&file, env::var("TARGET").ok().as_deref());
    assert_single_text_section(&file);
    reject_unwind_or_tls_sections(&file);
    validate_relocations(&file, "Rust stencil");
    for symbol in file.symbols() {
        if matches!(symbol.section(), SymbolSection::Undefined) {
            panic!(
                "Rust stencil has undeclared external symbol {:?}",
                symbol.name()
            );
        }
    }
    let symbol = file
        .symbols()
        .find(|symbol| {
            symbol
                .name()
                .ok()
                .is_some_and(|value| value.trim_start_matches('_') == name)
        })
        .expect("Rust stencil entry symbol must exist");
    let section_index = symbol.section_index().expect("stencil symbol section");
    let section = file
        .sections()
        .find(|section| section.index() == section_index)
        .expect("Rust stencil symbol section must exist");
    assert_eq!(
        section.kind(),
        SectionKind::Text,
        "Rust stencil entry must point into executable text"
    );
    let bytes = section.uncompressed_data().expect("read Rust stencil text");
    assert_eq!(
        section.size() as usize,
        bytes.len(),
        "Rust stencil text size is ambiguous"
    );
    if matches!(file.architecture(), object::Architecture::Aarch64) {
        assert_eq!(section.align() % 4, 0, "AArch64 text alignment is invalid");
    }
    let symbols = file
        .symbols()
        .filter(|symbol| symbol.section_index() == Some(section_index))
        .filter(|symbol| {
            symbol
                .name()
                .ok()
                .is_some_and(|value| value.trim_start_matches('_') == name)
        })
        .collect::<Vec<_>>();
    assert_eq!(symbols.len(), 1, "Rust stencil entry symbol must be unique");
    assert_no_other_global_text_symbols(&file, section_index, name);
    let symbol = &symbols[0];
    let offset = symbol
        .address()
        .checked_sub(section.address())
        .expect("stencil symbol precedes text");
    let size = symbol.size();
    assert!(offset == 0, "Rust stencil entry is not at section start");
    assert!(
        size == 0 || size as usize == bytes.len(),
        "Rust stencil has ambiguous symbol bounds"
    );
    let output = bytes.to_vec();
    assert!(
        !output.is_empty(),
        "Rust stencil has empty instruction bounds"
    );
    if matches!(file.architecture(), object::Architecture::Aarch64) {
        assert_eq!(
            output.len() % 4,
            0,
            "AArch64 instruction bounds are invalid"
        );
    }
    output
}

fn parse_object_range(
    path: &Path,
    name: &str,
    end_name: &str,
    expected_relocations: &[ExpectedRelocation],
    expected_holes: &[ExtractedHole],
) -> ParsedFragment {
    let data = fs::read(path).expect("read Rust fragment object");
    let file = object::File::parse(&*data).expect("parse Rust fragment object");
    assert_target_architecture(&file, env::var("TARGET").ok().as_deref());
    assert_target_format(&file, env::var("TARGET").ok().as_deref());
    reject_unwind_or_tls_sections(&file);
    let relocations = validate_fragment_relocations(&file, expected_relocations, "Rust fragment");
    let (start_section, start_address) = find_text_symbol(&file, name);
    let (end_section, end_address) = find_text_symbol(&file, end_name);
    assert_eq!(start_section, end_section, "fragment bounds cross sections");
    let section_index = start_section.expect("fragment start section");
    let section = file
        .sections()
        .find(|section| section.index() == section_index)
        .expect("fragment text section");
    assert_eq!(
        section.kind(),
        SectionKind::Text,
        "fragment is not executable text"
    );
    let start_offset = start_address
        .checked_sub(section.address())
        .expect("fragment start precedes section") as usize;
    let end_offset = end_address
        .checked_sub(section.address())
        .expect("fragment end precedes section") as usize;
    assert!(
        start_offset < end_offset,
        "fragment bounds are empty or reversed"
    );
    let bytes = section.uncompressed_data().expect("read fragment text");
    assert!(end_offset <= bytes.len(), "fragment end exceeds section");
    assert_eq!(start_offset, 0, "fragment entry is not at section start");
    let output = bytes[start_offset..end_offset].to_vec();
    assert!(!output.is_empty(), "fragment has empty instruction bounds");
    if matches!(file.architecture(), object::Architecture::Aarch64) {
        assert_eq!(output.len() % 4, 0, "AArch64 fragment bounds are invalid");
    }
    assert_no_other_global_text_symbols(&file, section_index, name);
    validate_fragment_holes(
        &file,
        section_index,
        start_address,
        name,
        &output,
        expected_holes,
    );
    let mut holes = expected_holes.to_vec();
    holes.extend(relocations.iter().map(|relocation| ExtractedHole {
        offset: relocation.offset,
        kind: relocation.kind,
    }));
    holes.sort_by_key(|hole| hole.offset);
    ParsedFragment {
        bytes: output,
        relocations,
        holes,
    }
}

fn expected_holes(declaration: &RegionDeclaration, target: &str) -> Vec<ExtractedHole> {
    let holes = if target.starts_with("aarch64") {
        declaration.aarch64_holes
    } else {
        declaration.holes
    };
    holes
        .iter()
        .map(|(offset, width, kind)| {
            assert_eq!(
                (*width, *kind),
                (8, "Literal64"),
                "unsupported assembly hole"
            );
            ExtractedHole {
                offset: *offset,
                kind: *kind,
            }
        })
        .collect()
}

fn validate_fragment_holes(
    file: &object::File<'_>,
    section: object::SectionIndex,
    start: u64,
    entry_name: &str,
    bytes: &[u8],
    holes: &[ExtractedHole],
) {
    for (index, hole) in holes.iter().enumerate() {
        let symbol = format!("{}_hole_{index}", entry_name.trim_start_matches('_'));
        let (hole_section, address) = find_text_symbol(file, &symbol);
        assert_eq!(hole_section, Some(section), "literal hole crosses sections");
        assert_eq!(
            address.checked_sub(start),
            Some(u64::from(hole.offset)),
            "literal hole offset drift"
        );
        let offset = usize::from(hole.offset);
        let slot = bytes
            .get(offset..offset + 8)
            .expect("literal hole exceeds fragment bounds");
        assert_eq!(slot, &[0; 8], "literal hole is not zeroed");
    }
}

fn validate_fragment_relocations(
    file: &object::File<'_>,
    expected: &[ExpectedRelocation],
    context: &str,
) -> Vec<ExtractedRelocation> {
    let mut records = Vec::new();
    if let object::File::MachO64(macho) = file {
        let mut relocation_count = 0;
        let mut consumed = vec![false; expected.len()];
        for section in macho.sections() {
            let text_section = section.kind() == SectionKind::Text;
            for relocation in section
                .macho_relocations()
                .expect("read Mach-O fragment relocations")
            {
                assert!(
                    text_section,
                    "Mach-O fragment relocation targets non-text data"
                );
                let info = relocation.info(Endianness::Little);
                assert!(
                    info.r_address <= u32::from(u16::MAX),
                    "Mach-O relocation offset is outside the declared range"
                );
                let target = macho
                    .symbol_by_index(object::SymbolIndex(info.r_symbolnum as usize))
                    .expect("Mach-O fragment relocation symbol")
                    .name()
                    .expect("Mach-O fragment relocation name")
                    .trim_start_matches('_');
                assert!(
                    info.r_extern,
                    "Mach-O fragment relocation must name a symbol"
                );
                let expected_index = expected_relocation_index(
                    expected,
                    &consumed,
                    section.kind(),
                    u64::from(info.r_address),
                    "Branch26",
                    target,
                )
                .expect("undeclared or duplicate Mach-O fragment relocation");
                let item = expected[expected_index];
                assert_eq!(item.addend, 0, "Mach-O branch addend must be zero");
                assert_eq!(item.width, 4, "Mach-O branch hole width must be four bytes");
                assert_eq!(info.r_type, object::macho::ARM64_RELOC_BRANCH26);
                assert!(info.r_pcrel && info.r_length == 2);
                let bytes = section
                    .uncompressed_data()
                    .expect("read Mach-O relocation section");
                let start = usize::try_from(info.r_address).expect("nonnegative relocation offset");
                let instruction = bytes
                    .get(start..start + 4)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u32::from_le_bytes)
                    .expect("Mach-O branch relocation lies within text");
                assert_eq!(
                    instruction & 0x03ff_ffff,
                    0,
                    "Mach-O Branch26 implicit addend must be zero"
                );
                consumed[expected_index] = true;
                records.push(ExtractedRelocation {
                    offset: item.offset,
                    kind: item.kind,
                    target: item.target,
                    addend: item.addend,
                });
                relocation_count += 1;
            }
        }
        assert_eq!(
            relocation_count,
            expected.len(),
            "{context} relocation count mismatch"
        );
        assert!(
            consumed.into_iter().all(|matched| matched),
            "{context} missing relocation"
        );
        records.sort_by_key(|relocation| relocation.offset);
        return records;
    }
    let text_index = file
        .sections()
        .find(|section| section.kind() == SectionKind::Text)
        .map(|section| section.index());
    let mut consumed = vec![false; expected.len()];
    for (relocation_section, offset, relocation) in file.sections().flat_map(|section| {
        section
            .relocations()
            .map(move |(offset, relocation)| (section.index(), offset, relocation))
    }) {
        let Some(text_section) = text_index else {
            panic!("{context} relocation has no text section")
        };
        assert_eq!(
            relocation_section, text_section,
            "fragment relocation section mismatch"
        );
        let target_name = match relocation.target() {
            RelocationTarget::Symbol(index) => file
                .symbol_by_index(index)
                .expect("fragment relocation symbol")
                .name()
                .expect("fragment relocation symbol name")
                .trim_start_matches('_')
                .to_owned(),
            _ => panic!("{context} relocation does not name a declared symbol"),
        };
        let expected_index = expected_relocation_index(
            expected,
            &consumed,
            SectionKind::Text,
            offset,
            "Branch26",
            &target_name,
        )
        .unwrap_or_else(|| panic!("{context} relocation at {offset:#x} is undeclared"));
        let item = expected[expected_index];
        consumed[expected_index] = true;
        assert_eq!(item.kind, "Branch26");
        assert_eq!(item.width, 4, "branch hole width must be four bytes");
        assert_eq!(item.addend, relocation.addend());
        assert_eq!(u64::from(item.offset), offset);
        assert_eq!(relocation.kind(), RelocationKind::PltRelative);
        assert_eq!(relocation.encoding(), RelocationEncoding::AArch64Call);
        assert_eq!(relocation.size(), 26);
        records.push(ExtractedRelocation {
            offset: item.offset,
            kind: item.kind,
            target: item.target,
            addend: item.addend,
        });
    }
    let relocation_count = file
        .sections()
        .map(|section| section.relocations().count())
        .sum::<usize>();
    assert_eq!(
        relocation_count,
        expected.len(),
        "{context} relocation count mismatch"
    );
    assert!(
        consumed.into_iter().all(|matched| matched),
        "{context} missing relocation"
    );
    records.sort_by_key(|relocation| relocation.offset);
    records
}

fn find_text_symbol<'data>(
    file: &object::File<'data>,
    name: &str,
) -> (Option<object::SectionIndex>, u64) {
    let mut symbols = file
        .symbols()
        .filter(|symbol| {
            symbol
                .name()
                .ok()
                .is_some_and(|value| value.trim_start_matches('_') == name)
        })
        .filter(|symbol| symbol.section_index().is_some())
        .collect::<Vec<_>>();
    assert_eq!(symbols.len(), 1, "fragment symbol {name} must be unique");
    let symbol = symbols.pop().expect("fragment symbol exists");
    (symbol.section_index(), symbol.address())
}

fn assert_no_other_global_text_symbols<'data>(
    file: &object::File<'data>,
    section_index: object::SectionIndex,
    entry_name: &str,
) {
    for symbol in file.symbols() {
        if symbol.section_index() != Some(section_index) || !symbol.is_global() {
            continue;
        }
        let name = symbol.name().unwrap_or_default().trim_start_matches('_');
        assert_eq!(
            name, entry_name,
            "Rust stencil has another global text symbol {name:?}"
        );
    }
}

fn assert_single_text_section<'data>(file: &object::File<'data>) {
    let text_sections = file
        .sections()
        .filter(|section| section.kind() == SectionKind::Text)
        .count();
    assert_eq!(
        text_sections, 1,
        "Rust stencil must have one executable text section"
    );
}

fn assert_target_architecture<'data>(file: &object::File<'data>, target: Option<&str>) {
    let expected = target.map(|triple| {
        if triple.starts_with("aarch64") {
            object::Architecture::Aarch64
        } else if triple.starts_with("x86_64") {
            object::Architecture::X86_64
        } else {
            panic!("unsupported stencil artifact target {triple}");
        }
    });
    if let Some(expected) = expected {
        assert_eq!(
            file.architecture(),
            expected,
            "Rust stencil target architecture mismatch"
        );
    }
}

fn assert_target_format<'data>(file: &object::File<'data>, target: Option<&str>) {
    let Some(target) = target else { return };
    let expected = if target.contains("-apple-") {
        BinaryFormat::MachO
    } else if target.contains("-windows-") {
        BinaryFormat::Coff
    } else {
        BinaryFormat::Elf
    };
    assert_eq!(
        file.format(),
        expected,
        "Rust stencil object format does not match target {target}"
    );
}

fn reject_unwind_or_tls_sections<'data>(file: &object::File<'data>) {
    for section in file.sections() {
        let name = section.name().unwrap_or_default();
        assert!(
            !matches!(name, ".eh_frame" | "__eh_frame" | ".gcc_except_table"),
            "Rust leaf carries an unsupported unwind section {name}"
        );
        assert!(
            !matches!(
                section.kind(),
                SectionKind::Tls | SectionKind::UninitializedTls | SectionKind::TlsVariables
            ),
            "Rust leaf carries unsupported TLS data"
        );
    }
}

/// Validate every relocation, including local references.  Isolated Rust
/// leaves currently declare no holes, so any relocation is rejected rather
/// than silently dropping a local literal/helper reference. Composable
/// templates will pass a catalog-derived allowlist here when their relocation
/// records are represented in `Stencil` metadata.
fn validate_relocations<'data>(file: &object::File<'data>, context: &str) {
    for section in file.sections() {
        for (offset, relocation) in section.relocations() {
            panic!(
                "{context} contains unsupported relocation at {offset:#x}: kind={:?} encoding={:?} size={}",
                relocation.kind(),
                relocation.encoding(),
                relocation.size()
            );
        }
    }
}
