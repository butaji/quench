use super::*;

#[test]
fn shared_slab_reclaims_oldest_idle_generation_at_global_cap() {
    let mut pool = SharedStencilSlab::new(MAX_ARENA_BYTES).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    static BYTES: [u8; MAX_ARENA_BYTES] = [0; MAX_ARENA_BYTES];
    let stencil = Stencil {
        bytes: &BYTES,
        holes: &[],
    };
    let mut first_owner = None;
    for raw_key in 0..=MAX_SHARED_SLAB_BYTES / MAX_ARENA_BYTES {
        let key = crate::stencil_fact::RegionKey(200 + raw_key as u64);
        if raw_key == MAX_SHARED_SLAB_BYTES / MAX_ARENA_BYTES {
            pool.active_dispatches.set(1);
            assert_eq!(
                pool.render_or_get(&mut cache, key, &stencil, &values),
                Err(ArenaError::Exhausted)
            );
            pool.active_dispatches.set(0);
        }
        let address = pool
            .render_or_get(&mut cache, key, &stencil, &values)
            .unwrap();
        if raw_key == 0 {
            first_owner = pool.owner_for(address);
        }
        pool.make_executable(address).unwrap();
    }
    assert_eq!(pool.capacity(), MAX_SHARED_SLAB_BYTES);
    assert_eq!(pool.slab_count(), 4);
    assert_eq!(
        first_owner
            .and_then(|owner| { cache.get_owned(crate::stencil_fact::RegionKey(200), 0, owner) }),
        None,
        "global-cap reclamation must prune retired owner rows"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn scalar_entry_signatures_are_not_interchangeable() {
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let cases = [
        (
            crate::stencil_select::loop_region_key(),
            crate::stencil_select::RegionAbi::ScalarF64Binary,
            false,
        ),
        (
            crate::stencil_select::add_chain_region_key(),
            crate::stencil_select::RegionAbi::ScalarF64x3,
            true,
        ),
    ];
    for (key, abi, is_three_input) in cases {
        let view = crate::stencil_select::select_physical_for_abi(key, abi).unwrap();
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let address = arena
            .render_physical_view_or_get(&mut cache, view, &values)
            .unwrap();
        arena.make_executable().unwrap();
        assert_eq!(arena.f64x3_entry(address).is_ok(), is_three_input);
        assert_eq!(arena.f64_entry(address).is_ok(), !is_three_input);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn owned_entry_rejects_generation_mismatch_before_invocation() {
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).expect("add key");
    let record = crate::stencil_select::select_region(key).expect("add row");
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = pool
        .render_or_get(&mut cache, key, &record.stencil, &values)
        .unwrap();
    pool.make_executable(address).unwrap();
    let entry = pool.f64_entry(address).unwrap();
    let owner = pool.owner_for(address).unwrap();
    let stale = EntryToken {
        address,
        entry_address: entry as usize,
        owner: owner.wrapping_add(1),
        abi: crate::stencil_select::RegionAbi::ScalarF64Binary,
        entry,
    };
    assert!(pool
        .with_owned(stale, |_| panic!("stale entry was invoked"))
        .is_err());
    let live = EntryToken {
        address,
        entry_address: entry as usize,
        owner,
        abi: crate::stencil_select::RegionAbi::ScalarF64Binary,
        entry,
    };
    let wrong_abi = EntryToken {
        address,
        entry_address: entry as usize,
        owner,
        abi: crate::stencil_select::RegionAbi::ScalarBool,
        entry,
    };
    assert!(pool
        .with_owned(wrong_abi, |_| panic!("wrong ABI entry was invoked"))
        .is_err());
    let value = pool.with_owned(live, |entry| entry(2.0, 3.0)).unwrap();
    assert_eq!(value, 5.0);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn dispatch_lease_rejects_stale_generation_before_entry() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(
        SharedStencilSlab::new(4096).expect("slab"),
    ));
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).expect("add key");
    let record = crate::stencil_select::select_region(key).expect("add row");
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let address = shared
        .borrow_mut()
        .render_or_get(
            &mut RenderedRegionCache::new(),
            key,
            &record.stencil,
            &PatchValues::from_site(&site),
        )
        .expect("render");
    shared
        .borrow_mut()
        .make_executable(address)
        .expect("publish");
    let owner = shared.borrow().owner_for(address).expect("owner");
    let state = std::rc::Rc::new(LeaseState {
        active: Cell::new(1),
        peak: Cell::new(1),
        owners: RefCell::new(HashMap::from([(owner.wrapping_add(1), 1)])),
        retired: RefCell::new(HashSet::new()),
    });
    let lease = AllocationLease {
        owner: std::rc::Rc::clone(&shared),
        state,
        address,
        owner_id: owner.wrapping_add(1),
        abi: crate::stencil_select::RegionAbi::ScalarF64Binary,
    };
    assert!(lease
        .invoke_dispatch(std::ptr::NonNull::<u8>::dangling().as_ptr().cast())
        .is_err());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn owned_entry_rejects_pointer_address_pairing_within_owner() {
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).expect("add key");
    let record = crate::stencil_select::select_region(key).expect("add row");
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let first = pool
        .render_or_get(&mut cache, key, &record.stencil, &values)
        .unwrap();
    pool.make_executable(first).unwrap();
    let second_key = crate::stencil_fact::RegionKey(key.value().wrapping_add(1));
    let second = pool
        .render_or_get(&mut cache, second_key, &record.stencil, &values)
        .unwrap();
    pool.make_executable(second).unwrap();
    let first_token = pool.owned_f64_entry(first).unwrap();
    let forged = EntryToken {
        address: second,
        ..first_token
    };
    assert!(pool
        .with_owned(forged, |_| panic!("mismatched entry was invoked"))
        .is_err());
}

#[cfg(target_arch = "aarch64")]
#[test]
fn shared_slab_accounts_active_native_execution() {
    let key = crate::stencil_select::array_numeric_loop_region_key();
    let record = crate::stencil_select::select_region(key).expect("array loop row");
    let site = QuickeningSite::<2>::new(Opcode::LoadLocal);
    let values = PatchValues::from_site(&site);
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = pool
        .render_or_get(&mut cache, key, &record.stencil, &values)
        .unwrap();
    pool.make_executable(address).unwrap();
    let mut data = vec![2.0, 3.0];
    let interrupt = std::sync::atomic::AtomicBool::new(false);
    let mut raw = crate::vm::NativeArrayLoopContext {
        data: data.as_mut_ptr(),
        len: data.len(),
        index: 0,
        end: data.len(),
        addend: 1.0,
        result: 0.0,
        interrupt: &interrupt,
    };
    assert_eq!(pool.active_dispatches(), 0);
    assert_eq!(
        pool.execute_dispatch_with_abi(
            address,
            (&mut raw as *mut crate::vm::NativeArrayLoopContext).cast(),
            crate::stencil_select::RegionAbi::ArrayNumericLoop,
        )
        .unwrap(),
        1
    );
    assert_eq!(pool.active_dispatches(), 0);
    assert_eq!(pool.peak_dispatches(), 1);
    assert_eq!(data, vec![3.0, 4.0]);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn dispatch_requires_declared_abi_and_published_entry() {
    extern "C" fn probe(context: *mut std::ffi::c_void) -> u64 {
        assert!(!context.is_null());
        0xA11Bu64
    }
    let key = crate::stencil_select::dispatch_region_key();
    let record = crate::stencil_select::select_region(key).expect("array loop row");
    let site = QuickeningSite::<2>::new(Opcode::Move);
    let values = PatchValues::from_site(&site).with_pointer_bits(probe as *const () as usize);
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = pool
        .render_or_get(&mut cache, key, &record.stencil, &values)
        .unwrap();
    pool.make_executable(address).unwrap();
    let mut marker = 0u8;
    let context = (&mut marker as *mut u8).cast::<std::ffi::c_void>();
    assert_eq!(pool.execute_dispatch(address, context).unwrap(), 0xA11B);
    assert!(pool
        .execute_dispatch_with_abi(
            address,
            context,
            crate::stencil_select::RegionAbi::ArrayKernel
        )
        .is_err());
    assert!(pool
        .execute_dispatch_with_abi(address + 1, context, record.abi)
        .is_err());
}
