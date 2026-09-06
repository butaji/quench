#[cfg(test)]
mod tests {
    use super::*;

    fn declaration(entries: &'static [u32]) -> RegionDeclaration {
        RegionDeclaration {
            name: "identity_probe",
            operations: &["Add", "Return"],
            abi: super::super::DeclAbi::ScalarF64Binary,
            x86_bytes: &[1],
            aarch64_bytes: &[2],
            portable_bytes: &[3],
            holes: &[],
            aarch64_holes: &[],
            entry: 0,
            external_entries: entries,
        }
    }

    fn extracted(bytes: &[u8]) -> ExtractedObject {
        ExtractedObject {
            bytes: bytes.to_vec(),
            fallthrough: None,
            relocations: Vec::new(),
            holes: Vec::new(),
        }
    }

    fn expected(offset: u16, target: &'static str) -> ExpectedRelocation {
        ExpectedRelocation {
            section: SectionKind::Text,
            offset: u64::from(offset),
            width: 4,
            kind: "Branch26",
            target,
            addend: 0,
        }
    }

    fn observed(offset: u64, target: &str) -> ObservedRelocation {
        ObservedRelocation {
            section: SectionKind::Text,
            offset,
            width: 4,
            kind: "Branch26",
            target: target.to_owned(),
            addend: 0,
        }
    }

    fn macho_branch_object(addend: i64) -> Vec<u8> {
        use object::write::{Object, Relocation, StandardSection, Symbol, SymbolSection};
        let mut output = Object::new(
            BinaryFormat::MachO,
            object::Architecture::Aarch64,
            object::Endianness::Little,
        );
        let text = output.section_id(StandardSection::Text);
        output.append_section_data(text, &0x9400_0000_u32.to_le_bytes(), 4);
        let target = output.add_symbol(Symbol {
            name: b"q_tail".to_vec(),
            value: 0,
            size: 0,
            kind: object::SymbolKind::Text,
            scope: object::SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Undefined,
            flags: object::SymbolFlags::None,
        });
        output
            .add_relocation(
                text,
                Relocation {
                    offset: 0,
                    symbol: target,
                    addend,
                    flags: RelocationFlags::Generic {
                        kind: RelocationKind::PltRelative,
                        encoding: RelocationEncoding::AArch64Call,
                        size: 26,
                    },
                },
            )
            .expect("write branch relocation");
        output.write().expect("write Mach-O fixture")
    }

    #[test]
    fn relocation_transaction_accepts_reordered_two_hole_input() {
        let expected = [expected(4, "first"), expected(12, "second")];
        let observed = [observed(12, "second"), observed(4, "first")];
        let records = match_relocation_observations(&expected, &observed).expect("match");
        assert_eq!(
            records.iter().map(|item| item.offset).collect::<Vec<_>>(),
            [4, 12]
        );
    }

    #[test]
    fn relocation_transaction_rejects_missing_hole() {
        let expected = [expected(4, "first"), expected(12, "second")];
        assert_eq!(
            match_relocation_observations(&expected, &[observed(4, "first")]),
            Err(RelocationContractError::Missing { offset: 12 })
        );
    }

    #[test]
    fn relocation_transaction_rejects_duplicate_hole() {
        let expected = [expected(4, "first"), expected(12, "second")];
        let duplicate = [observed(4, "first"), observed(4, "first")];
        assert_eq!(
            match_relocation_observations(&expected, &duplicate),
            Err(RelocationContractError::Duplicate { offset: 4 })
        );
    }

    #[test]
    fn relocation_transaction_rejects_unknown_target() {
        let expected = [expected(4, "first"), expected(12, "second")];
        let unknown = [observed(4, "first"), observed(12, "other")];
        assert_eq!(
            match_relocation_observations(&expected, &unknown),
            Err(RelocationContractError::Unknown { offset: 12 })
        );
    }

    #[test]
    fn relocation_transaction_rejects_addend_width_and_overlap_drift() {
        let contract = [expected(4, "first")];
        let mut drift = observed(4, "first");
        drift.addend = 4;
        assert_eq!(
            match_relocation_observations(&contract, &[drift]),
            Err(RelocationContractError::Addend {
                offset: 4,
                expected: 0,
                actual: 4
            })
        );
        let mut wide = observed(4, "first");
        wide.width = 8;
        assert_eq!(
            match_relocation_observations(&contract, &[wide]),
            Err(RelocationContractError::Width {
                offset: 4,
                expected: 4,
                actual: 8
            })
        );
        let overlap = [expected(4, "first"), expected(6, "second")];
        assert_eq!(
            match_relocation_observations(&overlap, &[]),
            Err(RelocationContractError::Overlap)
        );
    }

    #[test]
    fn macho_adapter_preserves_paired_branch_addend() {
        let bytes = macho_branch_object(4);
        let file = object::File::parse(&*bytes).expect("parse Mach-O fixture");
        let object::File::MachO64(file) = file else {
            panic!("fixture must be Mach-O64")
        };
        let observed_records = observe_macho_relocations(&file);
        assert_eq!(
            observed_records,
            [ObservedRelocation {
                addend: 4,
                ..observed(0, "q_tail")
            }]
        );
        let mut expected = expected(0, "q_tail");
        expected.addend = 4;
        assert_eq!(
            match_relocation_observations(&[expected.clone()], &observed_records),
            Ok(vec![expected])
        );
    }

    #[test]
    fn artifact_identity_covers_entries_and_extracted_payload() {
        static ENTRY_ZERO: [u32; 1] = [0];
        static ENTRY_ZERO_FOUR: [u32; 2] = [0, 4];
        let base = declaration(&ENTRY_ZERO);
        let changed_entries = declaration(&ENTRY_ZERO_FOUR);
        let identity = artifact_fingerprint(&base, "aarch64-test", "build", &extracted(&[1, 2]));
        let entry_identity = artifact_fingerprint(
            &changed_entries,
            "aarch64-test",
            "build",
            &extracted(&[1, 2]),
        );
        let byte_identity =
            artifact_fingerprint(&base, "aarch64-test", "build", &extracted(&[1, 3]));
        assert_ne!(identity, entry_identity);
        assert_ne!(identity, byte_identity);
    }

    #[test]
    fn artifact_identity_and_rendering_cover_verified_holes() {
        static ENTRY_ZERO: [u32; 1] = [0];
        let declaration = declaration(&ENTRY_ZERO);
        let plain = extracted(&[1, 2, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mut patched = extracted(&plain.bytes);
        patched.holes.push(ExtractedHole {
            offset: 2,
            kind: "Literal64",
        });
        let plain_id = artifact_fingerprint(&declaration, "aarch64-test", "build", &plain);
        let patched_id = artifact_fingerprint(&declaration, "aarch64-test", "build", &patched);
        assert_ne!(plain_id, patched_id);
        assert_eq!(holes_expr(&plain), "&[]");
        assert!(holes_expr(&patched).contains("offset: 2"));
        assert!(holes_expr(&patched).contains("HoleKind::Literal64"));
    }

    #[test]
    fn artifact_identity_covers_declared_physical_bindings() {
        static OPERATIONS: [&str; 19] = [
            "LoadLocal",
            "LoadConst",
            "Binary",
            "JumpIfFalse",
            "LoadLocal",
            "Move",
            "LoadLocal",
            "Move",
            "LoadLocal",
            "Slow",
            "LoadLocal",
            "AGetI",
            "AddConst",
            "ASetI",
            "Move",
            "LoadLocal",
            "AddConst",
            "StoreLocal",
            "Jump",
        ];
        let unbound = RegionDeclaration {
            name: "identity_probe",
            operations: &OPERATIONS,
            abi: super::super::DeclAbi::ArrayNumericLoop,
            x86_bytes: &[],
            aarch64_bytes: &[],
            portable_bytes: &[],
            holes: &[],
            aarch64_holes: &[],
            entry: 0,
            external_entries: &[0],
        };
        let bound = RegionDeclaration {
            name: "array_numeric_loop",
            ..unbound
        };
        assert!(super::super::rust_assembly_recipe(&bound)
            .is_some_and(|recipe| !recipe.bindings().is_empty()));
        let payload = extracted(&[1, 2, 3, 4]);
        assert_ne!(
            artifact_fingerprint(&bound, "aarch64-test", "build", &payload),
            artifact_fingerprint(&unbound, "aarch64-test", "build", &payload)
        );
    }
}
