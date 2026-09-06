#[cfg(test)]
mod tests {
    use super::*;

    fn declaration(entries: &'static [u32]) -> RegionDeclaration {
        RegionDeclaration {
            name: "identity_probe",
            operations: &["Add", "Return"],
            abi: super::super::DeclAbi::Scalar,
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
}
