use super::*;

#[test]
fn arena_enforces_a_bounded_mapping() {
    let mut arena = StencilArena::new(4096).unwrap();
    assert!(matches!(
        StencilArena::new(MAX_ARENA_BYTES + 1),
        Err(ArenaError::InvalidCapacity)
    ));
    assert_eq!(arena.capacity(), 4096);
    assert_eq!(arena.alloc(4097), Err(ArenaError::Exhausted));
    assert_eq!(arena.alloc(16), Ok(0));
    assert_eq!(arena.used(), 16);
}

#[test]
fn copy_patch_is_atomic_from_the_callers_view() {
    let mut arena = StencilArena::new(4096).unwrap();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    let stencil = Stencil {
        bytes: &[1, 2, 3, 4],
        holes: &[Hole {
            offset: 0,
            kind: HoleKind::Imm32,
        }],
    };
    let offset = arena.copy_and_patch(&stencil, &values).unwrap();
    assert_eq!(offset, 0);
    assert_eq!(arena.byte(0), 0);
}

#[test]
fn rendered_entries_start_on_target_instruction_boundaries() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    let first = Stencil {
        bytes: &[1, 2, 3],
        holes: &[],
    };
    let second = Stencil {
        bytes: &[4, 5, 6, 7],
        holes: &[],
    };
    arena
        .render_or_get(
            &mut cache,
            crate::stencil_fact::RegionKey(201),
            &first,
            &values,
        )
        .unwrap();
    let address = arena
        .render_or_get(
            &mut cache,
            crate::stencil_fact::RegionKey(202),
            &second,
            &values,
        )
        .unwrap();
    assert_eq!(address % STENCIL_ALIGNMENT, 0);
}

#[test]
fn render_cache_hit_does_not_allocate_again() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = crate::stencil_select::RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    let stencil = Stencil {
        bytes: &[1, 2, 3],
        holes: &[],
    };
    let key = crate::stencil_fact::RegionKey(7);
    let first = arena
        .render_or_get(&mut cache, key, &stencil, &values)
        .unwrap();
    let used = arena.used();
    let second = arena
        .render_or_get(&mut cache, key, &stencil, &values)
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(arena.used(), used);
}

#[test]
fn rendered_cache_reuses_unpatchable_quickening_state() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let mut first_site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let second_site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let shape = crate::shape_cache::ShapeId(1);
    let property = crate::shape_cache::PropertyId(2);
    assert!(matches!(
        first_site.observe(shape, property, 7),
        crate::quickening::QuickeningDecision::InstallGuard { .. }
    ));
    let first_values = PatchValues::from_site(&first_site);
    let second_values = PatchValues::from_site(&second_site);
    let stencil = Stencil {
        bytes: &[1, 2, 3],
        holes: &[],
    };
    let key = crate::stencil_fact::RegionKey(12);
    let first = arena
        .render_or_get(&mut cache, key, &stencil, &first_values)
        .unwrap();
    let used = arena.used();
    let second = arena
        .render_or_get(&mut cache, key, &stencil, &second_values)
        .unwrap();
    assert_ne!(first_values.signature(), second_values.signature());
    assert_eq!(first, second);
    assert_eq!(arena.used(), used);
}

#[test]
fn cache_entries_from_another_arena_are_not_executed() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    let stencil = Stencil {
        bytes: &[1, 2, 3],
        holes: &[],
    };
    let key = crate::stencil_fact::RegionKey(9);
    cache.insert(key, values.signature(), usize::MAX);
    let address = arena
        .render_or_get(&mut cache, key, &stencil, &values)
        .unwrap();
    assert_ne!(address, usize::MAX);
    assert_eq!(arena.used(), 3);
}

#[test]
fn rendered_entries_are_owned_by_their_arena_generation() {
    let mut first = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    let stencil = Stencil {
        bytes: &[1, 2, 3],
        holes: &[],
    };
    let key = crate::stencil_fact::RegionKey(91);
    let first_address = first
        .render_or_get(&mut cache, key, &stencil, &values)
        .unwrap();
    let first_owner = first.id();
    assert_eq!(cache.get_owned(key, 0, first_owner), Some(first_address));

    // A cache may outlive a disposable arena.  A new owner must not treat
    // the old raw address as callable, even if the OS later recycles the
    // same virtual mapping.
    drop(first);
    let mut second = StencilArena::new(4096).unwrap();
    assert_ne!(first_owner, second.id());
    assert_eq!(cache.get_owned(key, 0, second.id()), None);
    let second_address = second
        .render_or_get(&mut cache, key, &stencil, &values)
        .unwrap();
    assert_eq!(second.used(), stencil.bytes.len());
    assert_eq!(cache.get_owned(key, 0, second.id()), Some(second_address));
}

#[test]
fn shared_slab_rotates_only_after_publication_and_stays_bounded() {
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    static BYTES: [u8; 4096] = [0; 4096];
    let stencil = Stencil {
        bytes: &BYTES,
        holes: &[],
    };
    let first = pool
        .render_or_get(
            &mut cache,
            crate::stencil_fact::RegionKey(101),
            &stencil,
            &values,
        )
        .unwrap();
    pool.make_executable(first).unwrap();
    let second = pool
        .render_or_get(
            &mut cache,
            crate::stencil_fact::RegionKey(102),
            &stencil,
            &values,
        )
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(pool.slab_count(), 2);
    assert_eq!(pool.capacity(), 8192);
    assert!(pool.capacity() <= MAX_SHARED_SLAB_BYTES);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn shared_slab_reuses_equivalent_views_across_plan_caches() {
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut first_cache = RenderedRegionCache::new();
    let mut second_cache = RenderedRegionCache::new();
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).unwrap();
    let view = crate::stencil_select::select_physical(key).unwrap();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let first = pool
        .render_physical_view_or_get(&mut first_cache, view, &values)
        .unwrap();
    let used = pool.used();
    let second = pool
        .render_physical_view_or_get(&mut second_cache, view, &values)
        .unwrap();
    assert_eq!(second, first);
    assert_eq!(pool.used(), used);
    let owner = pool.owner_for(first).unwrap();
    let signature = view.cache_signature(&values);
    assert_eq!(second_cache.get_owned(key, signature, owner), Some(first));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn cache_cannot_relabel_a_published_entry() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let add = crate::stencil_select::select_physical(
        crate::stencil_select::numeric_region_key(Opcode::Add).unwrap(),
    )
    .unwrap();
    let multiply =
        crate::stencil_select::select_physical(crate::stencil_select::multiply_region_key())
            .unwrap();
    let _add_address = arena
        .render_physical_view_or_get(&mut cache, add, &values)
        .unwrap();
    let multiply_address = arena
        .render_physical_view_or_get(&mut cache, multiply, &values)
        .unwrap();
    arena.make_executable().unwrap();
    cache.insert_owned(
        add.key,
        add.cache_signature(&values),
        multiply_address,
        arena.id(),
    );

    assert_eq!(
        arena.render_physical_view_or_get(&mut cache, add, &values),
        Err(ArenaError::ProtectionFailed)
    );
    assert!(arena.f64_entry(multiply_address).is_ok());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn shared_slab_separates_distinct_patch_signatures() {
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut first_cache = RenderedRegionCache::new();
    let mut second_cache = RenderedRegionCache::new();
    let key = crate::stencil_fact::RegionKey(u64::MAX - 7);
    static BYTES: [u8; 8] = [0; 8];
    static HOLES: [crate::stencil_fact::Hole; 1] = [crate::stencil_fact::Hole {
        offset: 0,
        kind: crate::stencil_fact::HoleKind::Literal64,
    }];
    let stencil = Stencil {
        bytes: &BYTES,
        holes: &HOLES,
    };
    let site = QuickeningSite::<2>::new(Opcode::AddConst);
    let first_values = PatchValues::from_site(&site).with_constant_bits(1.0_f64.to_bits());
    let second_values = PatchValues::from_site(&site).with_constant_bits(2.0_f64.to_bits());
    let first = pool
        .render_or_get(&mut first_cache, key, &stencil, &first_values)
        .unwrap();
    let second = pool
        .render_or_get(&mut second_cache, key, &stencil, &second_values)
        .unwrap();
    assert_ne!(second, first);
}

#[test]
fn shared_slab_rejects_an_oversized_render_without_publishing() {
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    static TOO_LARGE: [u8; 8192] = [0; 8192];
    let stencil = Stencil {
        bytes: &TOO_LARGE,
        holes: &[],
    };
    assert_eq!(
        pool.render_or_get(
            &mut cache,
            crate::stencil_fact::RegionKey(103),
            &stencil,
            &values
        ),
        Err(ArenaError::Exhausted)
    );
    assert_eq!(pool.slab_count(), 0);
    assert_eq!(cache.len(), 0);
}

#[test]
fn shared_slab_evicts_only_idle_generations_without_reusing_cache_addresses() {
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetProperty);
    let values = PatchValues::from_site(&site);
    static BYTES: [u8; 4096] = [0; 4096];
    let stencil = Stencil {
        bytes: &BYTES,
        holes: &[],
    };
    let first_key = crate::stencil_fact::RegionKey(104);
    let second_key = crate::stencil_fact::RegionKey(105);
    let first = pool
        .render_or_get(&mut cache, first_key, &stencil, &values)
        .unwrap();
    let first_owner = pool.owner_for(first).unwrap();
    pool.make_executable(first).unwrap();
    pool.render_or_get(&mut cache, second_key, &stencil, &values)
        .unwrap();
    pool.active_dispatches.set(1);
    assert_eq!(pool.evict_idle(1), 0);
    pool.active_dispatches.set(0);
    assert_eq!(pool.evict_idle(1), 1);
    assert_eq!(pool.slab_count(), 1);
    assert_eq!(pool.evict_idle(1), 0);
    let replacement = pool
        .render_or_get(&mut cache, first_key, &stencil, &values)
        .unwrap();
    assert_ne!(pool.owner_for(replacement), Some(first_owner));
}

#[test]
fn cache_rows_are_pruned_with_retired_slab_owners() {
    let mut pool = SharedStencilSlab::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    static BYTES: [u8; 4096] = [0; 4096];
    let stencil = Stencil {
        bytes: &BYTES,
        holes: &[],
    };
    let first_key = crate::stencil_fact::RegionKey(107);
    let second_key = crate::stencil_fact::RegionKey(108);
    let first = pool
        .render_or_get(&mut cache, first_key, &stencil, &values)
        .unwrap();
    pool.render_or_get(&mut cache, second_key, &stencil, &values)
        .unwrap();
    let first_owner = pool.owner_for(first).unwrap();
    assert_eq!(cache.len(), 2);
    assert_eq!(pool.evict_idle_with_cache(&mut cache, 1), 1);
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.get_owned(first_key, 0, first_owner), None);
}
