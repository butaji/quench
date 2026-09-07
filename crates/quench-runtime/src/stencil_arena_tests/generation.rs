use super::*;

#[test]
fn physical_cache_signature_includes_selected_artifact_identity() {
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).expect("add view");
    let view = crate::stencil_select::select_physical(key).expect("add view");
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let mut alternate = view;
    alternate.artifact_id = "different-artifact";
    assert_ne!(
        view.cache_signature(&values),
        alternate.cache_signature(&values)
    );
}

#[cfg(quench_generated_stencil_artifacts)]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn generated_add_const_artifact_executes_through_typed_leaf_entry() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::AddConst);
    let values = PatchValues::from_site(&site);
    let key = crate::stencil_select::add_const_region_key();
    let view = crate::stencil_select::select_physical(key).expect("generated physical view");
    assert!(view.generated, "test must use the generated artifact");
    assert_eq!(
        view.record.abi,
        crate::stencil_select::RegionAbi::ScalarF64Binary
    );
    assert_eq!(view.abi, crate::stencil_select::RegionAbi::ScalarF64Binary);
    assert_eq!(view.key, key);
    assert!(view.target.is_some_and(|target| !target.is_empty()));
    assert!(view
        .fingerprint
        .is_some_and(|fingerprint| !fingerprint.is_empty()));
    assert_ne!(view.stencil.bytes, view.record.stencil.bytes);
    assert_eq!(
        arena.render_selected_f64(&mut cache, key, &values, 3.5, 2.25, || Ok(0.0)),
        Ok(5.75)
    );
    let witness = arena
        .last_physical_execution()
        .expect("successful entry witness");
    assert_eq!(witness.key, key);
    assert_eq!(witness.name, view.record.name);
    assert!(witness.generated);
    assert_eq!(witness.fingerprint, view.fingerprint);
    assert_eq!(witness.abi, view.abi);
    assert_eq!(witness.entry, view.entry);
    assert_eq!(witness.byte_len, view.stencil.bytes.len());
}

#[cfg(quench_generated_stencil_artifacts)]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn generated_add_chain_artifact_executes_with_selected_abi_view() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let key = crate::stencil_select::add_chain_region_key();
    let view = crate::stencil_select::select_physical(key).expect("generated chain view");
    assert!(view.generated, "chain must use generated artifact");
    assert_eq!(view.abi, crate::stencil_select::RegionAbi::ScalarF64x3);
    let tail = view.fallthrough.expect("generated chain tail");
    assert_eq!(view.relocations.len(), 1);
    assert_eq!(view.relocations[0].offset, 4);
    assert_eq!(view.relocations[0].target, "q_add_chain_tail");
    assert_eq!(view.relocations[0].addend, 0);
    assert_eq!(view.entry, 0);
    assert_eq!(view.external_entries, &[0]);
    assert_eq!(
        arena.render_selected_f64x3(&mut cache, key, &values, 1.5, 2.25, 3.0),
        Ok(6.75)
    );
    let witness = arena
        .last_physical_execution()
        .expect("generated chain execution witness");
    assert_eq!(witness.key, key);
    assert!(witness.generated);
    assert_eq!(witness.fingerprint, view.fingerprint);
    assert_eq!(witness.abi, view.abi);
    assert_eq!(
        witness.byte_len,
        view.stencil.bytes.len() + tail.stencil.bytes.len()
    );
    assert_eq!(arena.used(), witness.byte_len);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn generated_dispatch_entry_calls_the_patched_bridge() {
    extern "C" fn probe(context: *mut std::ffi::c_void) -> u64 {
        assert!(!context.is_null());
        0xD15A_7C1u64
    }
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Move);
    let values = PatchValues::from_site(&site).with_pointer_bits(probe as *const () as usize);
    let key = crate::stencil_select::dispatch_region_key();
    let record = crate::stencil_select::select_region(key).expect("dispatch catalog row");
    let address = arena
        .render_or_get(&mut cache, key, &record.stencil, &values)
        .unwrap();
    arena.make_executable().unwrap();
    let mut marker = 0u8;
    let status = arena
        .execute_dispatch(address, (&mut marker as *mut u8).cast())
        .unwrap();
    assert_eq!(status, 0xD15A_7C1u64);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn allocation_lease_dispatch_releases_pool_borrow_before_helper_reentry() {
    struct Reentry<'a> {
        pool: &'a std::rc::Rc<std::cell::RefCell<SharedStencilSlab>>,
        nested: EntryToken<extern "C" fn(f64, f64) -> f64>,
        effects: std::cell::Cell<u32>,
    }

    extern "C" fn helper(context: *mut std::ffi::c_void) -> u64 {
        let state = unsafe { &*(context as *const Reentry<'_>) };
        state.effects.set(state.effects.get().saturating_add(1));
        let Ok(nested) = SharedStencilSlab::acquire_owned(state.pool, state.nested) else {
            return 1;
        };
        {
            let mut pool = state.pool.borrow_mut();
            if pool.active_leases() != 2 || pool.evict_idle(0) != 0 {
                return 2;
            }
            if render_fresh_during_reentry(&mut pool).is_err() {
                return 3;
            }
        }
        if nested.invoke(|entry| entry(2.0, 3.0)) != Ok(5.0) {
            return 4;
        }
        if state.pool.borrow().active_leases() != 1 {
            return 5;
        }
        0xB0B0_7E5u64
    }

    fn render_fresh_during_reentry(pool: &mut SharedStencilSlab) -> Result<usize, ArenaError> {
        let fresh = Stencil {
            bytes: &[0, 0, 0, 0],
            holes: &[],
        };
        let site = QuickeningSite::<2>::new(Opcode::Add);
        pool.render_or_get(
            &mut RenderedRegionCache::new(),
            crate::stencil_fact::RegionKey(0xbee),
            &fresh,
            &PatchValues::from_site(&site),
        )
    }

    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        SharedStencilSlab::new(4096).unwrap(),
    ));
    let key = crate::stencil_select::dispatch_region_key();
    let record = crate::stencil_select::select_region(key).unwrap();
    let site = QuickeningSite::<2>::new(Opcode::Move);
    let values = PatchValues::from_site(&site).with_pointer_bits(helper as *const () as usize);
    let address = shared
        .borrow_mut()
        .render_or_get(
            &mut RenderedRegionCache::new(),
            key,
            &record.stencil,
            &values,
        )
        .unwrap();
    shared.borrow_mut().make_executable(address).unwrap();
    let nested_key = crate::stencil_select::numeric_region_key(Opcode::Add).unwrap();
    let nested_record = crate::stencil_select::select_region(nested_key).unwrap();
    let nested_address = shared
        .borrow_mut()
        .render_or_get(
            &mut RenderedRegionCache::new(),
            nested_key,
            &nested_record.stencil,
            &PatchValues::from_site(&QuickeningSite::<2>::new(Opcode::Add)),
        )
        .unwrap();
    shared.borrow_mut().make_executable(nested_address).unwrap();
    let nested = shared.borrow().owned_f64_entry(nested_address).unwrap();
    let state = Reentry {
        pool: &shared,
        nested,
        effects: std::cell::Cell::new(0),
    };
    let lease = SharedStencilSlab::acquire_address_lease(
        &shared,
        address,
        crate::stencil_select::RegionAbi::Bridge,
    )
    .unwrap();
    let status = lease
        .invoke_dispatch((&state as *const Reentry<'_>).cast_mut().cast())
        .unwrap();
    assert_eq!(status, 0xB0B0_7E5u64);
    assert_eq!(state.effects.get(), 1, "helper side effect must run once");
    assert_eq!(shared.borrow().active_leases(), 0);
}

#[test]
fn render_failure_uses_complete_fallback() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = crate::stencil_select::RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    let stencil = Stencil {
        bytes: &[1],
        holes: &[Hole {
            offset: 0,
            kind: HoleKind::Ptr64,
        }],
    };
    let result = arena.render_or_get(
        &mut cache,
        crate::stencil_fact::RegionKey(8),
        &stencil,
        &values,
    );
    assert!(result.is_err());
    assert_eq!(arena.used(), 0);
}

#[test]
fn wrong_typed_entry_rejects_before_publication() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = crate::stencil_select::RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let result = arena.render_selected_bool(
        &mut cache,
        crate::stencil_select::numeric_region_key(Opcode::Add).unwrap(),
        &values,
        1.0,
        2.0,
    );
    assert!(matches!(result, Err(ArenaError::ProtectionFailed)));
    assert_eq!(cache.len(), 0);
    assert_eq!(arena.used(), 0);
}

#[test]
fn data_only_region_returns_to_ordinary_semantics_without_execution() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    let result = arena.render_selected_f64(
        &mut cache,
        crate::stencil_fact::RegionKey::from_opcodes(
            crate::stencil_fact::RegionId(2),
            &[crate::ir::Opcode::GetProperty],
        ),
        &values,
        1.0,
        2.0,
        || Ok(17.0),
    );
    assert_eq!(result, Ok(17.0));
    assert_eq!(cache.len(), 0);
}

#[test]
fn unknown_region_never_allocates_before_fallback() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    let result = arena.render_selected_f64(
        &mut cache,
        crate::stencil_fact::RegionKey(0),
        &values,
        1.0,
        2.0,
        || Ok(7.0),
    );
    assert_eq!(result, Ok(7.0));
    assert_eq!(arena.used(), 0);
}
