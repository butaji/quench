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
            offset,
            width: 4,
            kind: "Branch26",
            target,
            addend: 0,
        }
    }

    #[test]
    fn relocation_identity_matching_is_order_independent() {
        let records = [expected(12, "tail"), expected(4, "helper")];
        let mut consumed = [false; 2];
        let second = expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Text,
            4,
            "Branch26",
            "helper",
        )
        .expect("second record");
        consumed[second] = true;
        let first = expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Text,
            12,
            "Branch26",
            "tail",
        )
        .expect("first record");
        assert_eq!(first, 0);
        assert!(expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Text,
            4,
            "Branch26",
            "helper",
        )
        .is_none());
    }

    #[test]
    fn relocation_identity_matching_rejects_wrong_target_or_kind() {
        let records = [expected(8, "tail")];
        let consumed = [false];
        assert!(expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Text,
            8,
            "Branch26",
            "other",
        )
        .is_none());
        assert!(expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Text,
            8,
            "Page21",
            "tail",
        )
        .is_none());
        assert!(expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Data,
            8,
            "Branch26",
            "tail",
        )
        .is_none());
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
