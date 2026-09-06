use super::*;

#[test]
fn extracted_build_artifacts_match_canonical_contracts() {
    #[cfg(quench_generated_stencil_artifacts)]
    assert!(
        !BUILD_STENCIL_ARTIFACTS.is_empty(),
        "enabled Rust extraction must publish at least one artifact"
    );
    for artifact in BUILD_STENCIL_ARTIFACTS {
        let record = CANONICAL_REGION_TABLE
            .iter()
            .find(|record| record.name == artifact.name)
            .expect("artifact declaration has a catalog row");
        assert!(!artifact.bytes.is_empty());
        assert!(artifact.stencil.validate());
        if artifact.has_fallthrough {
            assert!(!artifact.stencil.holes.is_empty());
            assert!(artifact.fallthrough.is_some());
        } else {
            assert!(artifact.fallthrough.is_none());
        }
        assert!(!artifact.fingerprint.is_empty());
        assert!(artifact.artifact_id.starts_with(artifact.name));
        assert!(!artifact.target.is_empty());
        assert_eq!(artifact.abi, record.abi);
        assert_eq!(artifact.key, record.key);
        for relocation in artifact.relocations {
            assert!(artifact
                .stencil
                .holes
                .iter()
                .any(|hole| { hole.offset == relocation.offset && hole.kind == relocation.kind }));
            assert!(!relocation.target.is_empty());
            assert_eq!(
                relocation.addend, 0,
                "only zero-addend patches are supported"
            );
        }
        for hole in artifact.stencil.holes {
            assert!(
                hole.kind == crate::stencil_fact::HoleKind::Literal64
                    || artifact
                        .relocations
                        .iter()
                        .any(|relocation| relocation.offset == hole.offset)
            );
        }
    }
    if !BUILD_STENCIL_ARTIFACTS.is_empty() {
        let chain = BUILD_STENCIL_ARTIFACTS
            .iter()
            .find(|artifact| artifact.key == add_chain_region_key())
            .expect("Rust generation must include the fused arithmetic chain");
        let chain_record = CANONICAL_REGION_TABLE
            .iter()
            .find(|record| record.name == "add_chain")
            .expect("fused chain declaration");
        assert_eq!(chain.bytes, chain_record.stencil.bytes);
        assert_eq!(
            select_stencil(chain_record.key).map(|view| view.stencil.bytes),
            Some(chain.bytes),
            "normal selection must use the generated chain artifact"
        );
    }
}

#[test]
fn physical_relocations_reject_unsupported_addends() {
    static BYTES: [u8; 4] = 0x1400_0000u32.to_le_bytes();
    static HOLES: [crate::stencil_fact::Hole; 1] = [crate::stencil_fact::Hole {
        offset: 0,
        kind: crate::stencil_fact::HoleKind::Branch26,
    }];
    let stencil = Stencil {
        bytes: &BYTES,
        holes: &HOLES,
    };
    let relocation = PhysicalRelocation {
        offset: 0,
        kind: crate::stencil_fact::HoleKind::Branch26,
        target: "q_tail",
        addend: 4,
    };
    assert!(!relocations_match(stencil, &[relocation]));
}

#[test]
fn physical_view_rejects_layout_mismatch_before_entry() {
    static BYTES: &[u8] = &[0xC3];
    static WRONG_ENTRIES: &[u16] = &[1];
    const TARGET: &str = match option_env!("QUENCH_BUILD_TARGET") {
        Some(target) => target,
        None => "test",
    };
    static BAD_ENTRY: BuildStencilArtifact = BuildStencilArtifact {
        name: "add_const",
        artifact_id: "bad-entry",
        key: RegionKey(0),
        target: "test",
        compiler: "test",
        fingerprint: "test",
        abi: RegionAbi::ScalarF64Binary,
        entry: 1,
        external_entries: &[0],
        has_fallthrough: false,
        executable: true,
        template_calls_helper: false,
        bytes: BYTES,
        data: &[],
        relocations: &[],
        stencil: Stencil {
            bytes: BYTES,
            holes: &[],
        },
        fallthrough: None,
    };
    static BAD_ENTRIES: BuildStencilArtifact = BuildStencilArtifact {
        name: "add_const",
        artifact_id: "bad-entries",
        key: RegionKey(0),
        target: "test",
        compiler: "test",
        fingerprint: "test",
        abi: RegionAbi::ScalarF64Binary,
        entry: 0,
        external_entries: WRONG_ENTRIES,
        has_fallthrough: false,
        executable: true,
        template_calls_helper: false,
        bytes: BYTES,
        data: &[],
        relocations: &[],
        stencil: Stencil {
            bytes: BYTES,
            holes: &[],
        },
        fallthrough: None,
    };
    static BAD_LAYOUT: BuildStencilArtifact = BuildStencilArtifact {
        name: "add_const",
        artifact_id: "bad-layout",
        key: RegionKey(0),
        target: TARGET,
        compiler: "test",
        fingerprint: "test",
        abi: RegionAbi::ScalarF64Binary,
        entry: 0,
        external_entries: &[0],
        has_fallthrough: true,
        executable: true,
        template_calls_helper: false,
        bytes: BYTES,
        data: &[],
        relocations: &[],
        stencil: Stencil {
            bytes: BYTES,
            holes: &[],
        },
        fallthrough: None,
    };
    static BAD_ABI: BuildStencilArtifact = BuildStencilArtifact {
        name: "add_const",
        artifact_id: "bad-abi",
        key: RegionKey(0),
        target: TARGET,
        compiler: "test",
        fingerprint: "test",
        abi: RegionAbi::TaggedWord,
        entry: 0,
        external_entries: &[0],
        has_fallthrough: false,
        executable: true,
        template_calls_helper: false,
        bytes: BYTES,
        data: &[],
        relocations: &[],
        stencil: Stencil {
            bytes: BYTES,
            holes: &[],
        },
        fallthrough: None,
    };
    static BAD_RELOCATION: BuildStencilArtifact = BuildStencilArtifact {
        name: "add_const",
        artifact_id: "add_const@test",
        key: RegionKey(0),
        target: TARGET,
        compiler: "test",
        fingerprint: "test",
        abi: RegionAbi::ScalarF64Binary,
        entry: 0,
        external_entries: &[0],
        has_fallthrough: false,
        executable: true,
        template_calls_helper: false,
        bytes: BYTES,
        data: &[],
        relocations: &[PhysicalRelocation {
            offset: 4,
            kind: crate::stencil_fact::HoleKind::Branch26,
            target: "q_missing",
            addend: 0,
        }],
        stencil: Stencil {
            bytes: BYTES,
            holes: &[],
        },
        fallthrough: None,
    };
    let record = CANONICAL_REGION_TABLE
        .iter()
        .find(|record| record.name == "add_const")
        .expect("add_const row");
    assert!(!artifact_identity_matches(&BAD_ENTRY, record));
    assert!(generated_physical_view(record.key, record, &BAD_ENTRY).is_none());
    assert!(generated_physical_view(record.key, record, &BAD_ENTRIES).is_none());
    assert!(generated_physical_view(record.key, record, &BAD_LAYOUT).is_none());
    assert!(generated_physical_view(record.key, record, &BAD_ABI).is_none());
    assert!(generated_physical_view(record.key, record, &BAD_RELOCATION).is_none());

    static BAD_TARGET: BuildStencilArtifact = BuildStencilArtifact {
        name: "add_const",
        artifact_id: "bad-target",
        key: RegionKey(0),
        target: "mismatched-target",
        compiler: "test",
        fingerprint: "test",
        abi: RegionAbi::ScalarF64Binary,
        entry: 0,
        external_entries: &[0],
        has_fallthrough: false,
        executable: true,
        template_calls_helper: false,
        bytes: BYTES,
        data: &[],
        relocations: &[],
        stencil: Stencil {
            bytes: BYTES,
            holes: &[],
        },
        fallthrough: None,
    };
    assert!(generated_physical_view(record.key, record, &BAD_TARGET).is_none());
}

#[test]
fn typed_selection_carries_the_verified_view_and_rejects_wrong_abi() {
    let mut checked = 0;
    for record in CANONICAL_REGION_TABLE {
        let Some(view) = select_physical_for_abi(record.key, record.abi) else {
            continue;
        };
        assert_eq!(view.key, record.key);
        assert_eq!(view.record.name, record.name);
        assert_eq!(view.abi, record.abi);
        assert!(!view.stencil.bytes.is_empty());
        assert!(view.contract().abi_is_well_formed());
        checked += 1;
    }
    assert!(checked > 0, "catalog must expose a typed physical view");
    let scalar = CANONICAL_REGION_TABLE
        .iter()
        .find(|record| record.abi == RegionAbi::ScalarF64Binary)
        .expect("scalar catalog row");
    assert!(select_physical_for_abi(scalar.key, RegionAbi::TaggedWord).is_none());
}

#[cfg(quench_generated_stencil_artifacts)]
#[test]
fn duplicate_generated_artifacts_fail_closed() {
    let first = *BUILD_STENCIL_ARTIFACTS.first().expect("generated artifact");
    let duplicate = [first, first];
    assert!(unique_artifact(&duplicate, first.key, first.name).is_err());
}
