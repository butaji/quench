use super::*;

#[cfg(target_arch = "aarch64")]
#[test]
fn admission_rank_prefers_native_work_removed_over_bridge_and_size() {
    let record = |name| {
        region_records()
            .iter()
            .find(|record| record.name == name)
            .expect("canonical region")
    };
    let bridge = record("get_index");
    let native_get = record("array_get_number");
    let native_update = record("array_numeric_update");
    assert!(admission_rank(native_get) > admission_rank(bridge));
    assert!(admission_rank(native_update) > admission_rank(native_get));
}

#[test]
fn selection_is_canonical_and_misses_fall_back() {
    assert!(select_stencil(loop_region_key()).is_some());
    assert!(select_stencil(RegionKey(0)).is_none());
    assert_eq!(
        loop_region_key(),
        RegionKey::from_opcodes(
            loop_region_id(),
            &[crate::ir::Opcode::Add, crate::ir::Opcode::Return]
        )
    );
}

#[test]
fn numeric_leaf_keys_are_catalog_admissions() {
    for opcode in [
        crate::ir::Opcode::Add,
        crate::ir::Opcode::Sub,
        crate::ir::Opcode::Mul,
        crate::ir::Opcode::Div,
        crate::ir::Opcode::AddConst,
    ] {
        let key = numeric_region_key(opcode).expect("numeric leaf key");
        assert!(select_region(key).is_some());
    }
    assert_eq!(numeric_region_key(crate::ir::Opcode::GetProperty), None);
}

#[test]
fn numeric_add_leaf_never_selects_nonreturning_fallthrough_head() {
    let key = numeric_region_key(crate::ir::Opcode::Add).expect("add leaf");
    let record = select_region(key).expect("add declaration");
    assert_eq!(key, loop_region_key());
    assert!(record.fallthrough.is_none());
    assert!(fallthrough_region_key() != key);
}

#[test]
fn scalar_leaf_and_continuation_keys_are_distinct() {
    for opcode in [
        crate::ir::Opcode::Add,
        crate::ir::Opcode::Sub,
        crate::ir::Opcode::Mul,
        crate::ir::Opcode::Div,
    ] {
        let leaf = numeric_region_key(opcode).expect("numeric leaf");
        let continuation = continuation_region_key(opcode).expect("numeric continuation");
        assert_ne!(leaf, continuation);
        assert!(select_region(leaf).expect("leaf row").fallthrough.is_none());
        assert!(select_region(continuation)
            .expect("continuation row")
            .fallthrough
            .is_some());
        let view = select_physical(continuation).expect("continuation view");
        assert!(!view.links.is_empty());
        assert!(view
            .links
            .iter()
            .all(|link| link.role == SuccessorRole::Next));
        #[cfg(quench_generated_stencil_artifacts)]
        assert_eq!(view.links.len(), view.relocations.len());
    }
}

#[test]
fn successor_role_participates_in_physical_identity() {
    let view = select_physical(fallthrough_region_key()).expect("continuation view");
    let mut altered = view;
    let mut links = view.links.to_vec();
    links[0].role = SuccessorRole::False;
    altered.links = Box::leak(links.into_boxed_slice());
    let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::Add);
    let values = crate::stencil_fact::PatchValues::from_site(&site);
    assert!(!view.matches(&altered));
    assert_ne!(
        view.cache_signature(&values),
        altered.cache_signature(&values)
    );
}

#[test]
fn complete_physical_contract_participates_in_cache_identity() {
    let view = select_physical(fallthrough_region_key()).expect("continuation view");
    let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::Add);
    let values = crate::stencil_fact::PatchValues::from_site(&site);
    let signature = view.cache_signature(&values);
    let alternate_entry = PhysicalStencilView {
        entry: view.entry.saturating_add(4),
        ..view
    };
    let alternate_abi = PhysicalStencilView {
        abi: RegionAbi::TaggedWord,
        ..view
    };
    let alternate_target = PhysicalStencilView {
        target: Some("different-target"),
        ..view
    };
    for alternate in [alternate_entry, alternate_abi, alternate_target] {
        assert_ne!(signature, alternate.cache_signature(&values));
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[test]
fn numeric_rows_never_admit_x86_bytes_on_other_isas() {
    for opcode in [
        crate::ir::Opcode::Add,
        crate::ir::Opcode::Sub,
        crate::ir::Opcode::Mul,
        crate::ir::Opcode::Div,
        crate::ir::Opcode::AddConst,
    ] {
        let key = numeric_region_key(opcode).expect("numeric leaf key");
        assert!(!select_region(key).expect("catalog row").executable);
    }
}

#[test]
fn property_row_is_catalog_admitted() {
    let key = property_region_key();
    let record = select_region(key).expect("property admission row");
    assert_eq!(
        record.executable,
        cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
    );
    assert_eq!(
        record.stencil.bytes.len(),
        if cfg!(target_arch = "x86_64") {
            48
        } else if cfg!(target_arch = "aarch64") {
            80
        } else {
            1
        }
    );
    assert!(record.stencil.holes.is_empty());
}

#[test]
fn move_row_is_catalog_admitted() {
    let key = move_region_key();
    let record = select_region(key).expect("move admission row");
    assert_eq!(
        record.executable,
        cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
    );
    assert_eq!(
        record.stencil.bytes.len(),
        if cfg!(target_arch = "x86_64") {
            4
        } else if cfg!(target_arch = "aarch64") {
            8
        } else {
            1
        }
    );
    assert!(record.stencil.holes.is_empty());
}

#[test]
fn dispatch_row_covers_every_compact_opcode() {
    let record = select_region(dispatch_region_key()).expect("dispatch admission row");
    assert_eq!(
        record.operations.len(),
        usize::from(crate::ir::Opcode::COUNT)
    );
    for opcode in 1..=crate::ir::Opcode::COUNT {
        let opcode = crate::ir::Opcode::from_u8(opcode).expect("catalog opcode");
        assert!(record.operations.contains(&opcode));
    }
    assert_eq!(record.executable, cfg!(target_arch = "x86_64"));
    assert_eq!(
        record.stencil.holes.len(),
        if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
            1
        } else {
            0
        }
    );
}

#[test]
fn quickened_catalog_entries_use_the_same_cfg_checked_dispatch_region() {
    let record = select_region(dispatch_region_key()).expect("dispatch admission row");
    for opcode in [
        crate::ir::Opcode::GetPropertyQuickened,
        crate::ir::Opcode::GetNQuickened,
        crate::ir::Opcode::AGetIQuickened,
    ] {
        assert!(record.operations.contains(&opcode));
    }
}

#[test]
fn generated_accessor_matches_legacy_fallthrough_key() {
    // Explicit before/after migration check: the former hand-written
    // construction and the generated declaration are identical.
    let legacy = RegionKey::from_opcodes(
        fallthrough_region_id(),
        &[crate::ir::Opcode::Add, crate::ir::Opcode::Return],
    );
    assert_eq!(fallthrough_region_key(), legacy);
}

#[test]
fn dispatch_uses_region_sequence_and_falls_back_once() {
    let selected = dispatch_region(
        loop_region_key(),
        |record| Ok::<_, ()>(record.operations.len()),
        || Ok::<_, ()>(0),
    );
    assert_eq!(selected, Ok(2));
    let ordinary = dispatch_region(RegionKey(0), |_| Ok::<_, ()>(99), || Ok::<_, ()>(7));
    assert_eq!(ordinary, Ok(7));
}

#[test]
fn removing_a_failed_render_does_not_change_bounded_replacement_state() {
    let mut cache = RenderedRegionCache::new();
    let key = RegionKey(17);
    let signature = 23;
    cache.insert(key, signature, 41);
    assert_eq!(cache.len(), 1);
    assert!(cache.remove(key, signature, 41));
    assert_eq!(cache.get(key, signature), None);
    assert_eq!(cache.len(), 0);
    assert!(!cache.remove(key, signature, 41));
}

#[test]
fn reusable_type_pass_reduces_two_distinct_predicates() {
    let checks = [TypeCheck::Number, TypeCheck::Object];
    let first = reduce_type_checks(&checks, |check| {
        if check == TypeCheck::Number {
            PredicateResult::AlwaysTrue
        } else {
            PredicateResult::Unknown
        }
    });
    let second = reduce_type_checks(&checks, |check| {
        if check == TypeCheck::Object {
            PredicateResult::AlwaysFalse
        } else {
            PredicateResult::Unknown
        }
    });
    assert_eq!(first.as_slice(), &[Some(TypeCheck::Object)]);
    assert_eq!(first.always_true(), 1);
    assert_eq!(second.as_slice(), &[Some(TypeCheck::Number)]);
    assert_eq!(second.always_false(), 1);
}

#[test]
fn cfg_rejects_external_entry_into_region_interior() {
    let blocks = [
        RegionBlock {
            id: 10,
            predecessors: &[],
            external_entry: true,
        },
        RegionBlock {
            id: 11,
            predecessors: &[10],
            external_entry: false,
        },
    ];
    assert!(has_single_entry_point(10, &blocks));
    let bad = [
        RegionBlock {
            id: 10,
            predecessors: &[],
            external_entry: true,
        },
        RegionBlock {
            id: 11,
            predecessors: &[10],
            external_entry: true,
        },
    ];
    assert!(!has_single_entry_point(10, &bad));
}

#[test]
fn cfg_rejects_external_entry_into_multi_instruction_span_interior() {
    // A fused span must contain at least three operations for this check:
    // an entry at the final operation is still an externally reachable
    // interior entry and therefore cannot be rendered as one atomic
    // single-entry region.
    let blocks = [
        RegionBlock {
            id: 0,
            predecessors: &[],
            external_entry: true,
        },
        RegionBlock {
            id: 1,
            predecessors: &[0],
            external_entry: false,
        },
        RegionBlock {
            id: 2,
            predecessors: &[1, 9],
            external_entry: true,
        },
    ];
    assert!(!has_single_entry_point(0, &blocks));
}

#[test]
fn loop_body_span_is_single_entry_and_rejects_interior_edges() {
    let record = select_region(loop_body_region_key()).expect("loop body row");
    assert_eq!(record.operations.len(), 7);
    assert!(has_single_entry_point(
        u32::from(record.entry),
        &[RegionBlock {
            id: 0,
            predecessors: &[],
            external_entry: true,
        }]
    ));
    assert!(!has_single_entry_point(
        0,
        &[
            RegionBlock {
                id: 0,
                predecessors: &[],
                external_entry: true,
            },
            RegionBlock {
                id: 3,
                predecessors: &[2],
                external_entry: true,
            },
        ]
    ));
}

#[test]
fn rendered_region_memo_is_fixed_capacity() {
    let mut cache = RenderedRegionCache::new();
    assert_eq!(cache.allocated_entries(), 0);
    assert_eq!(cache.allocated_bytes(), 0);
    assert!(
        std::mem::size_of::<RenderedRegionCache>()
            < std::mem::size_of::<[Option<RenderedRegion>; MAX_RENDERED_REGIONS]>()
    );
    for index in 0..(MAX_RENDERED_REGIONS + 1) {
        cache.insert(RegionKey(index as u64), 0, index);
    }
    assert_eq!(cache.len(), MAX_RENDERED_REGIONS);
    assert_eq!(cache.get(RegionKey(0), 0), None);
    assert_eq!(
        cache.get(RegionKey(MAX_RENDERED_REGIONS as u64), 0),
        Some(MAX_RENDERED_REGIONS)
    );
    assert!(cache.allocated_entries() <= MAX_RENDERED_REGIONS);
    assert!(
        cache.allocated_bytes() <= MAX_RENDERED_REGIONS * std::mem::size_of::<RenderedRegion>()
    );
    cache.clear();
    assert_eq!(cache.allocated_entries(), 0);
    assert_eq!(cache.allocated_bytes(), 0);
}

#[test]
fn promotion_rule_is_fact_only_and_shared() {
    let first = RegionKey(1);
    let second = RegionKey(2);
    assert!(!promotion_admitted(first, first));
    assert!(promotion_admitted(first, second));
    assert_eq!(
        choose_promotion(Some(first), first, false),
        Promotion::Repatch
    );
    assert_eq!(
        choose_promotion(Some(first), second, true),
        Promotion::Repatch
    );
    assert_eq!(
        choose_promotion(Some(first), second, false),
        Promotion::Render
    );
}
