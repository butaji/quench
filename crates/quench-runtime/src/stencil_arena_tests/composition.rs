use super::*;

#[cfg(target_arch = "x86_64")]
#[test]
fn installed_region_falls_through_to_next_region() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let key = crate::stencil_select::fallthrough_region_key();
    let view = crate::stencil_select::select_physical(key).expect("fallthrough view");
    assert_eq!(
        arena.render_selected_f64(&mut cache, key, &values, 10.25, 2.5, || Ok(0.0)),
        Ok(12.75)
    );
    let tail = view.fallthrough.expect("fallthrough tail");
    assert_eq!(
        arena.used(),
        view.stencil.bytes.len() + tail.stencil.bytes.len()
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn labeled_three_fragment_region_executes_both_successors() {
    let bytes = three_fragment_layout();
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let view = crate::stencil_select::select_physical(crate::stencil_select::multiply_region_key())
        .expect("binary physical view");
    let image = VerifiedRegionImage::from_test_parts(view, 0, bytes.clone());
    let address = arena.publish_composed(&mut cache, &image).unwrap();
    assert_eq!(arena.execute_f64(address, 1.0, 2.0), Ok(5.0));
    assert_eq!(arena.used(), bytes.len());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn selected_fragments_form_a_reusable_three_fragment_chain() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let view =
        crate::stencil_select::select_physical(crate::stencil_select::fallthrough_region_key())
            .expect("linked fragment view");
    let image = crate::stencil_region_builder::compose_linear_chain(view, 2, &values)
        .expect("compose two additions and return");
    let address = arena
        .publish_region_image_or_get(&mut cache, &image)
        .expect("publish composed image");
    assert_eq!(arena.execute_f64(address, 1.5, 2.0), Ok(5.5));
    let used = arena.used();
    assert_eq!(
        arena.publish_region_image_or_get(&mut cache, &image),
        Ok(address)
    );
    assert_eq!(arena.used(), used);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn three_fragment_layout() -> Vec<u8> {
    use crate::stencil_layout::{Fragment, LabelId, StencilLayout};

    let (first, second, tail, kind, offset) = three_fragment_bytes();
    let fragments = [
        Fragment {
            label: LabelId(0),
            bytes: &first,
        },
        Fragment {
            label: LabelId(1),
            bytes: &second,
        },
        Fragment {
            label: LabelId(2),
            bytes: &tail,
        },
    ];
    let fixups = [
        successor_fixup(0, 1, offset, kind),
        successor_fixup(1, 2, offset, kind),
    ];
    let mut bytes = Vec::new();
    StencilLayout::new(&fragments, &fixups)
        .finalize_into(&mut bytes)
        .unwrap();
    bytes
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn successor_fixup(
    fragment: u8,
    target: u8,
    offset: u16,
    kind: FixupKind,
) -> crate::stencil_layout::Fixup {
    crate::stencil_layout::Fixup {
        fragment,
        offset,
        target: crate::stencil_layout::LabelId(target),
        addend: 0,
        kind,
    }
}

#[cfg(target_arch = "x86_64")]
fn three_fragment_bytes() -> (Vec<u8>, Vec<u8>, Vec<u8>, FixupKind, u16) {
    let body = vec![0xF2, 0x0F, 0x58, 0xC1, 0xE9, 0, 0, 0, 0];
    (body.clone(), body, vec![0xC3], FixupKind::X86Rel32, 5)
}

#[cfg(target_arch = "aarch64")]
fn three_fragment_bytes() -> (Vec<u8>, Vec<u8>, Vec<u8>, FixupKind, u16) {
    let mut body = 0x1E61_2800u32.to_le_bytes().to_vec();
    body.extend_from_slice(&0x1400_0000u32.to_le_bytes());
    let tail = 0xD65F_03C0u32.to_le_bytes().to_vec();
    (body.clone(), body, tail, FixupKind::Aarch64Branch26, 4)
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn generated_fallthrough_region_is_selected_by_canonical_key() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let key = crate::stencil_select::fallthrough_region_key();
    let view = crate::stencil_select::select_physical(key).expect("fallthrough view");
    assert_eq!(
        arena.render_selected_f64(&mut cache, key, &values, 1.25, 2.75, || Ok(0.0)),
        Ok(4.0)
    );
    let used = arena.used();
    assert_eq!(
        arena.render_selected_f64(&mut cache, key, &values, 3.5, 4.5, || Ok(0.0)),
        Ok(8.0)
    );
    assert_eq!(arena.used(), used);
    assert_eq!(cache.len(), 1);
    if view.generated {
        let tail = view.fallthrough.expect("generated successor");
        assert_eq!(
            arena.used(),
            view.stencil.bytes.len() + tail.stencil.bytes.len()
        );
        assert_eq!(view.relocations.len(), 2);
        assert!(view.relocations.iter().all(|relocation| {
            relocation.kind == HoleKind::Branch26 && relocation.target == "q_fallthrough_tail"
        }));
        assert!(view
            .relocations
            .iter()
            .any(|relocation| relocation.target == tail.target));
        assert!(view
            .relocations
            .iter()
            .any(|relocation| relocation.offset == 8));
    }
    let witness = arena
        .last_physical_execution()
        .expect("fallthrough must execute selected bytes");
    assert_eq!(witness.key, key);
    assert_eq!(witness.name, "fallthrough");
    assert_eq!(
        witness.abi,
        crate::stencil_select::RegionAbi::ScalarF64Binary
    );
    assert_eq!(witness.entry, view.entry);
    assert_eq!(
        witness.byte_len,
        view.stencil.bytes.len() + view.fallthrough.map_or(0, |item| item.stencil.bytes.len())
    );
    #[cfg(quench_generated_stencil_artifacts)]
    assert!(witness.generated);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn selected_chain_rejects_substituted_tail_before_publication() {
    static BAD_TAIL_BYTES: &[u8] = &[0xC3, 0xC3];
    static BAD_TAIL: Stencil = Stencil {
        bytes: BAD_TAIL_BYTES,
        holes: &[],
    };
    let key = crate::stencil_select::fallthrough_region_key();
    let view = crate::stencil_select::select_physical(key).expect("fallthrough view");
    let tail = view.fallthrough.expect("declared successor");
    let bad_view = crate::stencil_select::PhysicalStencilView {
        fallthrough: Some(crate::stencil_select::PhysicalFallthrough {
            stencil: &BAD_TAIL,
            target: tail.target,
        }),
        ..view
    };
    let mut arena = StencilArena::new(4096).expect("arena");
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let result = arena.render_physical_view_or_get(&mut cache, bad_view, &values);
    assert_eq!(result, Err(ArenaError::ProtectionFailed));
    assert_eq!(arena.used(), 0);
    assert_eq!(cache.len(), 0);
    let wrong_target = crate::stencil_select::PhysicalStencilView {
        fallthrough: Some(crate::stencil_select::PhysicalFallthrough {
            target: "q_unrelated_successor",
            ..tail
        }),
        ..view
    };
    let result = arena.render_physical_view_or_get(&mut cache, wrong_target, &values);
    assert_eq!(result, Err(ArenaError::ProtectionFailed));
    assert_eq!(arena.used(), 0);
    assert_eq!(cache.len(), 0);
    assert_eq!(tail.stencil.bytes, view.fallthrough.unwrap().stencil.bytes);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn selected_chain_rejects_unknown_identity_before_publication() {
    let key = crate::stencil_select::fallthrough_region_key();
    let view = crate::stencil_select::select_physical(key).expect("fallthrough view");
    let unknown = crate::stencil_select::PhysicalStencilView {
        key: crate::stencil_fact::RegionKey(u64::MAX),
        ..view
    };
    let mut arena = StencilArena::new(4096).expect("arena");
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let result = arena.render_physical_view_or_get(&mut cache, unknown, &values);
    assert_eq!(result, Err(ArenaError::ProtectionFailed));
    assert_eq!(arena.used(), 0);
    assert_eq!(cache.len(), 0);
}

#[cfg(all(quench_generated_stencil_artifacts, target_arch = "aarch64"))]
#[test]
fn generated_fallthrough_view_carries_declared_branch_and_tail() {
    let key = crate::stencil_select::fallthrough_region_key();
    let view = crate::stencil_select::select_physical(key).expect("physical view");
    assert!(view.generated);
    assert_eq!(view.stencil.holes.len(), 2);
    assert_eq!(view.stencil.holes[0].offset, 4);
    assert!(matches!(
        view.stencil.holes[0].kind,
        crate::stencil_fact::HoleKind::Branch26
    ));
    assert_eq!(view.relocations.len(), 2);
    let offsets = view
        .relocations
        .iter()
        .map(|relocation| relocation.offset)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(offsets, [4, 8].into_iter().collect());
    assert!(view
        .relocations
        .iter()
        .all(|relocation| relocation.target == "q_fallthrough_tail"));
    let tail = view.fallthrough.expect("generated tail");
    assert_eq!(tail.stencil.bytes, &[0xc0, 0x03, 0x5f, 0xd6]);
    assert!(tail.stencil.holes.is_empty());
}

#[cfg(all(quench_generated_stencil_artifacts, target_arch = "aarch64"))]
#[test]
fn generated_key_rejects_legacy_layout_before_publication() {
    let key = crate::stencil_select::add_const_region_key();
    let view = crate::stencil_select::select_physical(key).expect("physical view");
    let record = crate::stencil_select::select_region(key).expect("canonical row");
    assert!(view.generated);
    assert_ne!(view.stencil.bytes, record.stencil.bytes);
    let mut arena = StencilArena::new(4096).expect("arena");
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::AddConst);
    let values = PatchValues::from_site(&site);
    assert_eq!(
        arena.render_or_get(&mut cache, key, &record.stencil, &values),
        Err(ArenaError::ProtectionFailed)
    );
    assert_eq!(arena.used(), 0);
    assert_eq!(cache.len(), 0);
    let bad_data = crate::stencil_select::PhysicalStencilView {
        data: &[0xAA],
        ..view
    };
    assert_eq!(
        arena.render_physical_view_or_get(&mut cache, bad_data, &values),
        Err(ArenaError::ProtectionFailed)
    );
    assert_eq!(arena.used(), 0);
    assert_eq!(cache.len(), 0);
}

#[test]
fn physical_view_mismatch_is_rejected_before_allocation() {
    let key = crate::stencil_select::add_const_region_key();
    let view = crate::stencil_select::select_physical(key).expect("physical view");
    static BAD_TAIL_BYTES: &[u8] = &[0xc3];
    static BAD_TAIL: Stencil = Stencil {
        bytes: BAD_TAIL_BYTES,
        holes: &[],
    };
    let bad = crate::stencil_select::PhysicalStencilView {
        abi: crate::stencil_select::RegionAbi::TaggedWord,
        ..view
    };
    let site = QuickeningSite::<2>::new(Opcode::AddConst);
    let values = PatchValues::from_site(&site);
    let mut arena = StencilArena::new(4096).expect("arena");
    let mut cache = RenderedRegionCache::new();
    assert_eq!(
        arena.render_physical_view_or_get(&mut cache, bad, &values),
        Err(ArenaError::ProtectionFailed)
    );
    assert_eq!(arena.used(), 0);
    assert_eq!(cache.len(), 0);
    let bad_fallthrough = crate::stencil_select::PhysicalStencilView {
        fallthrough: Some(crate::stencil_select::PhysicalFallthrough {
            stencil: &BAD_TAIL,
            target: "q_unexpected_tail",
        }),
        ..view
    };
    assert_eq!(
        arena.render_physical_view_or_get(&mut cache, bad_fallthrough, &values),
        Err(ArenaError::ProtectionFailed)
    );
    assert_eq!(arena.used(), 0);
    assert_eq!(cache.len(), 0);
    let bad_entry = crate::stencil_select::PhysicalStencilView {
        entry: view.entry.saturating_add(1),
        ..view
    };
    assert_eq!(
        arena.render_physical_view_or_get(&mut cache, bad_entry, &values),
        Err(ArenaError::ProtectionFailed)
    );
    assert_eq!(arena.used(), 0);
    assert_eq!(cache.len(), 0);
}
