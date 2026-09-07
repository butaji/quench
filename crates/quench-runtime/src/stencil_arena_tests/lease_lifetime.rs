use super::*;

#[test]
fn shared_slab_typed_entry_guard_blocks_eviction_during_call() {
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    static BYTES: [u8; 4] = [0; 4];
    let address = pool
        .render_or_get(
            &mut cache,
            crate::stencil_fact::RegionKey(106),
            &Stencil {
                bytes: &BYTES,
                holes: &[],
            },
            &values,
        )
        .unwrap();
    assert_eq!(pool.active_dispatches(), 0);
    let observed = pool
        .with_active(address, || pool.active_dispatches())
        .unwrap();
    assert_eq!(observed, 1);
    assert_eq!(pool.active_dispatches(), 0);
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = pool.with_active(address, || panic!("simulated helper unwind"));
    }));
    assert!(unwind.is_err());
    assert_eq!(pool.active_dispatches(), 0);
    assert_eq!(pool.evict_idle(0), 1);
    // A cached scalar/composed pointer from the retired owner must fail
    // closed rather than being invoked after its slab is reclaimed.
    assert!(pool.with_active(address, || ()).is_err());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn retaining_lease_allows_reentrant_pool_access_and_delays_retirement() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        SharedStencilSlab::new(4096).expect("slab"),
    ));
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).expect("add key");
    let record = crate::stencil_select::select_region(key).expect("add row");
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let mut cache = RenderedRegionCache::new();
    let address = shared
        .borrow_mut()
        .render_or_get(&mut cache, key, &record.stencil, &values)
        .expect("render");
    shared
        .borrow_mut()
        .make_executable(address)
        .expect("publish");
    let token = shared
        .borrow()
        .owned_f64_entry(address)
        .expect("typed token");
    let lease = SharedStencilSlab::acquire_owned(&shared, token).expect("lease");
    let result = lease
        .invoke(|entry| {
            let mut pool = shared.borrow_mut();
            assert_eq!(pool.active_leases(), 1);
            assert_eq!(pool.evict_idle(0), 0);
            let mut cache = RenderedRegionCache::new();
            let fresh = Stencil {
                bytes: &[0, 0, 0, 0],
                holes: &[],
            };
            assert!(pool
                .render_or_get(
                    &mut cache,
                    crate::stencil_fact::RegionKey(0xabc),
                    &fresh,
                    &PatchValues::from_site(&QuickeningSite::<2>::new(Opcode::Add)),
                )
                .is_ok());
            entry(2.0, 3.0)
        })
        .expect("reentrant entry");
    assert_eq!(result, 5.0);
    assert_eq!(shared.borrow().active_leases(), 0);
    assert_eq!(shared.borrow_mut().evict_idle(0), 2);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn active_lease_allows_independent_idle_slab_reclamation() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        SharedStencilSlab::new(4096).expect("slab"),
    ));
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).unwrap();
    let record = crate::stencil_select::select_region(key).unwrap();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let mut cache = RenderedRegionCache::new();
    let address = shared
        .borrow_mut()
        .render_or_get(&mut cache, key, &record.stencil, &values)
        .unwrap();
    shared.borrow_mut().make_executable(address).unwrap();
    shared
        .borrow_mut()
        .render_or_get(
            &mut cache,
            crate::stencil_fact::RegionKey(0xbee),
            &Stencil {
                bytes: &[0],
                holes: &[],
            },
            &values,
        )
        .unwrap();
    assert_eq!(shared.borrow().slab_count(), 2);
    let token = shared.borrow().owned_f64_entry(address).unwrap();
    let lease = SharedStencilSlab::acquire_owned(&shared, token).unwrap();
    let result = lease.invoke(|entry| {
        assert_eq!(shared.borrow_mut().evict_idle_with_cache(&mut cache, 0), 1);
        assert_eq!(shared.borrow().capacity(), 4096);
        entry(4.0, 5.0)
    });
    assert_eq!(result, Ok(9.0));
    assert_eq!(shared.borrow_mut().evict_idle_with_cache(&mut cache, 0), 1);
    assert_eq!(shared.borrow().capacity(), 0);
    assert_eq!(cache.len(), 0);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn retire_live_add() -> (
    std::rc::Rc<std::cell::RefCell<SharedStencilSlab>>,
    RenderedRegionCache,
    OwnedLease<extern "C" fn(f64, f64) -> f64>,
    EntryToken<extern "C" fn(f64, f64) -> f64>,
    crate::stencil_select::PhysicalStencilView,
    usize,
) {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        SharedStencilSlab::new(4096).expect("slab"),
    ));
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).unwrap();
    let view = crate::stencil_select::select_physical(key).unwrap();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let mut cache = RenderedRegionCache::new();
    let address = shared
        .borrow_mut()
        .render_physical_view_or_get(&mut cache, view, &values)
        .unwrap();
    shared.borrow_mut().make_executable(address).unwrap();
    let token = shared.borrow().owned_f64_entry(address).unwrap();
    let lease = SharedStencilSlab::acquire_owned(&shared, token).unwrap();

    shared
        .borrow_mut()
        .retire_allocation(address, &mut cache)
        .unwrap();
    (shared, cache, lease, token, view, address)
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn retired_generation_stays_charged_until_final_lease_release() {
    let (shared, mut cache, lease, token, view, address) = retire_live_add();
    assert_eq!(cache.len(), 0, "retirement prunes the derived row");
    assert_eq!(
        shared.borrow().cache.len(),
        0,
        "retirement prunes the canonical row"
    );
    assert_eq!(
        shared.borrow().capacity(),
        4096,
        "active retired code stays charged"
    );
    assert!(shared.borrow().owned_f64_entry(address).is_err());
    assert!(SharedStencilSlab::acquire_owned(&shared, token).is_err());

    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let replacement = shared
        .borrow_mut()
        .render_physical_view_or_get(&mut cache, view, &values)
        .expect("retired generation must not block fresh admission");
    shared.borrow_mut().make_executable(replacement).unwrap();
    assert_ne!(replacement, address);
    assert_eq!(shared.borrow().capacity(), 8192);

    assert_eq!(lease.invoke(|entry| entry(4.0, 5.0)), Ok(9.0));
    assert_eq!(
        shared.borrow().capacity(),
        4096,
        "final lease reclaims only the retired generation"
    );
    assert_eq!(shared.borrow().slab_count(), 1);
    assert_eq!(shared.borrow_mut().evict_idle_with_cache(&mut cache, 0), 1);
    assert_eq!(shared.borrow().capacity(), 0);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn lease_retains_pool_after_last_external_owner_until_return() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        SharedStencilSlab::new(4096).unwrap(),
    ));
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).unwrap();
    let record = crate::stencil_select::select_region(key).unwrap();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let address = shared
        .borrow_mut()
        .render_or_get(
            &mut RenderedRegionCache::new(),
            key,
            &record.stencil,
            &PatchValues::from_site(&site),
        )
        .unwrap();
    shared.borrow_mut().make_executable(address).unwrap();
    let token = shared.borrow().owned_f64_entry(address).unwrap();
    let lease = SharedStencilSlab::acquire_owned(&shared, token).unwrap();
    let weak = std::rc::Rc::downgrade(&shared);
    drop(shared);
    let retained = weak.upgrade().expect("lease retains executable pool");
    assert_eq!(retained.borrow().capacity(), 4096);
    assert_eq!(retained.borrow().active_leases(), 1);
    drop(retained);
    assert_eq!(lease.invoke(|entry| entry(3.0, 6.0)), Ok(9.0));
    assert!(weak.upgrade().is_none(), "last lease releases the pool");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn raw_allocation_lease_protects_region_dispatch() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        SharedStencilSlab::new(4096).expect("slab"),
    ));
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).expect("add key");
    let record = crate::stencil_select::select_region(key).expect("add row");
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let mut cache = RenderedRegionCache::new();
    let address = shared
        .borrow_mut()
        .render_or_get(&mut cache, key, &record.stencil, &values)
        .expect("render");
    shared
        .borrow_mut()
        .make_executable(address)
        .expect("publish");
    let lease = SharedStencilSlab::acquire_address_lease(
        &shared,
        address,
        crate::stencil_select::RegionAbi::ScalarF64Binary,
    )
    .expect("allocation lease");
    let result = lease
        .invoke(|| {
            let mut pool = shared.borrow_mut();
            assert_eq!(pool.active_leases(), 1);
            assert_eq!(pool.evict_idle(0), 0);
            9_u64
        })
        .expect("region dispatch lease");
    assert_eq!(result, 9);
    assert_eq!(shared.borrow().active_leases(), 0);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn active_lease_preserves_own_code_while_reclaiming_idle_slabs() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        SharedStencilSlab::new(MAX_ARENA_BYTES).expect("slab"),
    ));
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).expect("add key");
    let record = crate::stencil_select::select_region(key).expect("add row");
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let mut cache = RenderedRegionCache::new();
    let address = shared
        .borrow_mut()
        .render_or_get(&mut cache, key, &record.stencil, &values)
        .expect("render");
    shared
        .borrow_mut()
        .make_executable(address)
        .expect("publish");
    let token = shared
        .borrow()
        .owned_f64_entry(address)
        .expect("typed token");
    let lease = SharedStencilSlab::acquire_owned(&shared, token).expect("lease");
    static FULL_SLAB: [u8; MAX_ARENA_BYTES] = [0; MAX_ARENA_BYTES];
    for raw_key in 0..3 {
        assert!(shared
            .borrow_mut()
            .render_or_get(
                &mut cache,
                crate::stencil_fact::RegionKey(0xfeed + raw_key),
                &Stencil {
                    bytes: &FULL_SLAB,
                    holes: &[],
                },
                &values,
            )
            .is_ok());
    }
    let result = shared.borrow_mut().render_or_get(
        &mut cache,
        crate::stencil_fact::RegionKey(0xfeed + 3),
        &Stencil {
            bytes: &FULL_SLAB,
            holes: &[],
        },
        &values,
    );
    assert!(result.is_ok());
    assert_eq!(shared.borrow().capacity(), MAX_SHARED_SLAB_BYTES);
    assert_eq!(lease.invoke(|entry| entry(8.0, 5.0)), Ok(13.0));
    assert_eq!(shared.borrow().active_leases(), 0);
}
