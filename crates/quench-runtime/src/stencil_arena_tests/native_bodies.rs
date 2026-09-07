use super::*;

fn render_selected<const N: usize>(
    arena: &mut StencilArena,
    cache: &mut RenderedRegionCache,
    key: crate::stencil_fact::RegionKey,
    values: &PatchValues<'_, N>,
) -> usize {
    let view = crate::stencil_select::select_physical(key).expect("physical stencil view");
    arena
        .render_physical_view_or_get(cache, view, values)
        .expect("render selected physical view")
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_add_region_matches_ordinary_number_semantics() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let key = crate::stencil_select::fallthrough_region_key();
    let view = crate::stencil_select::select_physical(key).expect("fallthrough view");
    assert_eq!(
        arena.render_selected_f64(&mut cache, key, &values, 20.5, 22.25, || Ok(7.0)),
        Ok(42.75)
    );
    assert_eq!(20.5_f64 + 22.25_f64, 42.75);

    let negative_zero = arena
        .render_selected_f64(&mut cache, key, &values, -0.0, -0.0, || Ok(1.0))
        .unwrap();
    assert_eq!(negative_zero.to_bits(), (-0.0_f64).to_bits());

    let nan = arena
        .render_selected_f64(&mut cache, key, &values, f64::NAN, 1.0, || Ok(1.0))
        .unwrap();
    assert!(nan.is_nan());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_primitive_constant_returns_patched_tagged_word() {
    let key = crate::stencil_select::load_const_region_key();
    let view = crate::stencil_select::select_physical(key).expect("constant declaration");
    let site = QuickeningSite::<2>::new(Opcode::LoadConst);
    let values = PatchValues::from_site(&site)
        .with_constant_bits(crate::tagged_value::TaggedValue::number(42.5).bits());
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = arena
        .render_physical_view_or_get(&mut cache, view, &values)
        .unwrap();
    arena.make_executable().unwrap();
    let entry = arena.constant_word_entry(address).unwrap();
    assert_eq!(
        entry(),
        crate::tagged_value::TaggedValue::number(42.5).bits()
    );
    #[cfg(quench_generated_stencil_artifacts)]
    {
        assert!(view.generated);
        assert_eq!(
            view.stencil.holes,
            &[Hole {
                offset: 8,
                kind: HoleKind::Literal64
            }]
        );
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn typed_entry_rejects_wrong_published_abi() {
    let key = crate::stencil_select::numeric_region_key(Opcode::Add).unwrap();
    let site = QuickeningSite::<2>::new(Opcode::Add);
    let values = PatchValues::from_site(&site);
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = render_selected(&mut arena, &mut cache, key, &values);
    arena.make_executable().unwrap();
    assert!(
        arena.bool_entry(address).is_err(),
        "scalar bytes cannot be called as bool ABI"
    );
    assert!(arena.f64_entry(address).is_ok());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_numeric_truthiness_matches_number_toboolean() {
    let key = crate::stencil_select::truthy_number_region_key();
    let site = QuickeningSite::<2>::new(Opcode::JumpIfFalse);
    let values = PatchValues::from_site(&site);
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = render_selected(&mut arena, &mut cache, key, &values);
    arena.make_executable().unwrap();
    let entry = arena.bool_unary_entry(address).unwrap();
    for (value, expected) in [
        (0.0, false),
        (-0.0, false),
        (f64::NAN, false),
        (f64::INFINITY, true),
        (-3.5, true),
    ] {
        assert_eq!(entry(value) != 0, expected);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_tagged_truthiness_matches_primitive_tags() {
    let key = crate::stencil_select::truthy_word_region_key();
    let view = crate::stencil_select::select_physical(key).expect("word truthiness row");
    let site = QuickeningSite::<2>::new(Opcode::JumpIfFalse);
    let values = PatchValues::from_site(&site)
        .with_constant_bits(crate::tagged_value::TaggedValue::bool(true).bits());
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = render_selected(&mut arena, &mut cache, key, &values);
    arena.make_executable().unwrap();
    let entry = arena.word_bool_entry(address).unwrap();
    assert!(entry(crate::tagged_value::TaggedValue::bool(true).bits()) != 0);
    assert_eq!(
        entry(crate::tagged_value::TaggedValue::bool(false).bits()) != 0,
        false
    );
    assert_eq!(
        entry(crate::tagged_value::TaggedValue::null().bits()) != 0,
        false
    );
    assert_eq!(
        entry(crate::tagged_value::TaggedValue::undefined().bits()) != 0,
        false
    );
    #[cfg(quench_generated_stencil_artifacts)]
    {
        assert!(view.generated);
        assert_eq!(
            view.stencil.holes,
            &[Hole {
                offset: 16,
                kind: HoleKind::Literal64
            }]
        );
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_nullish_word_uses_verified_literal_hole() {
    let key = crate::stencil_select::nullish_word_region_key();
    let view = crate::stencil_select::select_physical(key).expect("nullish word row");
    let site = QuickeningSite::<2>::new(Opcode::Unary);
    let values = PatchValues::from_site(&site)
        .with_constant_bits(crate::tagged_value::TaggedValue::undefined().bits());
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = render_selected(&mut arena, &mut cache, key, &values);
    arena.make_executable().unwrap();
    let entry = arena.word_bool_entry(address).unwrap();
    assert_ne!(entry(crate::tagged_value::TaggedValue::null().bits()), 0);
    assert_ne!(
        entry(crate::tagged_value::TaggedValue::undefined().bits()),
        0
    );
    assert_eq!(
        entry(crate::tagged_value::TaggedValue::bool(false).bits()),
        0
    );
    #[cfg(quench_generated_stencil_artifacts)]
    {
        assert!(view.generated);
        assert_eq!(
            view.stencil.holes,
            &[Hole {
                offset: 24,
                kind: HoleKind::Literal64
            }]
        );
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_tagged_pointer_truthiness_is_true() {
    let key = crate::stencil_select::truthy_pointer_word_region_key();
    let view = crate::stencil_select::select_physical(key).expect("pointer truthiness row");
    let site = QuickeningSite::<2>::new(Opcode::JumpIfFalse);
    let values = PatchValues::from_site(&site);
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = render_selected(&mut arena, &mut cache, key, &values);
    arena.make_executable().unwrap();
    let entry = arena.word_bool_entry(address).unwrap();
    let pointer = crate::tagged_value::TaggedValue::object_ptr(0x1000).unwrap();
    assert_ne!(entry(pointer.bits()), 0);
    #[cfg(quench_generated_stencil_artifacts)]
    assert!(view.generated);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_equality_region_matches_numeric_semantics() {
    let key = crate::stencil_select::compare_equal_region_key();
    let site = QuickeningSite::<2>::new(Opcode::Binary);
    let values = PatchValues::from_site(&site);
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = render_selected(&mut arena, &mut cache, key, &values);
    arena.make_executable().unwrap();
    assert!(arena.execute_bool(address, 4.0, 4.0).unwrap());
    assert!(!arena.execute_bool(address, 4.0, 5.0).unwrap());
    assert!(!arena.execute_bool(address, f64::NAN, f64::NAN).unwrap());
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_tagged_identity_equality_matches_non_numeric_values() {
    let key = crate::stencil_select::compare_equal_word_region_key();
    let site = QuickeningSite::<2>::new(Opcode::Binary);
    let values = PatchValues::from_site(&site);
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = render_selected(&mut arena, &mut cache, key, &values);
    arena.make_executable().unwrap();
    let entry = arena.word_pair_bool_entry(address).unwrap();
    let true_bits = crate::tagged_value::TaggedValue::bool(true).bits();
    let false_bits = crate::tagged_value::TaggedValue::bool(false).bits();
    let null_bits = crate::tagged_value::TaggedValue::null().bits();
    assert!(entry(true_bits, true_bits) != 0);
    assert!(entry(true_bits, false_bits) == 0);
    assert!(entry(null_bits, null_bits) != 0);

    let not_equal_key = crate::stencil_select::compare_not_equal_word_region_key();
    let mut not_equal_arena = StencilArena::new(4096).unwrap();
    let mut not_equal_cache = RenderedRegionCache::new();
    let not_equal_address = render_selected(
        &mut not_equal_arena,
        &mut not_equal_cache,
        not_equal_key,
        &values,
    );
    not_equal_arena.make_executable().unwrap();
    let not_equal = not_equal_arena
        .word_pair_bool_entry(not_equal_address)
        .unwrap();
    assert!(not_equal(true_bits, false_bits) != 0);
    assert!(not_equal(null_bits, null_bits) == 0);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_ordered_regions_reject_unordered_nan() {
    let site = QuickeningSite::<2>::new(Opcode::Binary);
    let values = PatchValues::from_site(&site);
    let cases = [
        (crate::stencil_select::compare_less_region_key(), 1.0, 2.0),
        (
            crate::stencil_select::compare_less_equal_region_key(),
            2.0,
            2.0,
        ),
        (
            crate::stencil_select::compare_greater_region_key(),
            2.0,
            1.0,
        ),
        (
            crate::stencil_select::compare_greater_equal_region_key(),
            2.0,
            2.0,
        ),
    ];
    for (key, lhs, rhs) in cases {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let address = render_selected(&mut arena, &mut cache, key, &values);
        arena.make_executable().unwrap();
        assert!(arena.execute_bool(address, lhs, rhs).unwrap());
        assert!(!arena.execute_bool(address, f64::NAN, rhs).unwrap());
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_i32_bitwise_regions_match_signed_results() {
    let site = QuickeningSite::<2>::new(Opcode::Binary);
    let values = PatchValues::from_site(&site);
    let cases = [
        (
            crate::stencil_select::bitwise_and_region_key(),
            0xF0F0_i32,
            0x0FF0_i32,
        ),
        (
            crate::stencil_select::bitwise_or_region_key(),
            0xF000_i32,
            0x00F0_i32,
        ),
        (
            crate::stencil_select::bitwise_xor_region_key(),
            -1_i32,
            0x0F0F_i32,
        ),
    ];
    for (key, lhs, rhs) in cases {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let address = render_selected(&mut arena, &mut cache, key, &values);
        arena.make_executable().unwrap();
        let expected = match key {
            key if key == crate::stencil_select::bitwise_and_region_key() => lhs & rhs,
            key if key == crate::stencil_select::bitwise_or_region_key() => lhs | rhs,
            _ => lhs ^ rhs,
        };
        assert_eq!(arena.execute_i32(address, lhs, rhs).unwrap(), expected);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_shift_regions_mask_counts_and_preserve_unsigned_result() {
    let site = QuickeningSite::<2>::new(Opcode::Binary);
    let values = PatchValues::from_site(&site);
    for (key, lhs, rhs, expected) in [
        (
            crate::stencil_select::shift_left_region_key(),
            1_i32,
            32_i32,
            1_i32,
        ),
        (
            crate::stencil_select::shift_right_region_key(),
            -8_i32,
            1_i32,
            -4_i32,
        ),
    ] {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let address = render_selected(&mut arena, &mut cache, key, &values);
        arena.make_executable().unwrap();
        assert_eq!(arena.execute_i32(address, lhs, rhs).unwrap(), expected);
    }
    let key = crate::stencil_select::shift_right_zero_region_key();
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let address = render_selected(&mut arena, &mut cache, key, &values);
    arena.make_executable().unwrap();
    assert_eq!(arena.execute_u32(address, u32::MAX, 1), Ok(2_147_483_647));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_add_const_region_uses_patched_constant_data() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::AddConst);
    let values = PatchValues::from_site(&site).with_constant_bits(2.5_f64.to_bits());
    let key = crate::stencil_select::add_const_region_key();
    let view = crate::stencil_select::select_physical(key).expect("add-const view");
    let result = arena.render_selected_f64(&mut cache, key, &values, 4.0, 0.0, || Ok(99.0));
    assert_eq!(result, Ok(6.5));
    assert_eq!(arena.used(), view.stencil.bytes.len());
    if !view.generated {
        assert_eq!(
            arena.byte(if cfg!(target_arch = "aarch64") {
                16
            } else {
                13
            }),
            0
        );
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_property_leaf_guards_layout_and_loads_tagged_word() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::GetN);
    let values = PatchValues::from_site(&site);
    let key = crate::stencil_select::property_region_key();
    let view = crate::stencil_select::select_physical_for_abi(
        key,
        crate::stencil_select::RegionAbi::PropertyGuard,
    )
    .expect("property view");
    let address = arena
        .render_physical_view_or_get(&mut cache, view, &values)
        .unwrap();
    arena.make_executable().unwrap();
    let object =
        crate::value::ObjectData::new(vec![("value".into(), crate::value::Value::Number(42.5))]);
    let access = object
        .guarded_plain_slot(object.semantic_layout_id(), 0, "value")
        .expect("plain slot");
    let mut context = crate::native_property::NativePropertyReadContext::new(access);
    let entry = arena.property_guard_entry(address).expect("typed entry");
    let status = entry(&mut context);
    assert_eq!(
        context.result(status),
        Some(crate::tagged_value::TaggedValue::number(42.5).bits())
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn executable_property_write_has_distinct_abi_and_commits_word() {
    let mut arena = StencilArena::new(4096).unwrap();
    let mut cache = RenderedRegionCache::new();
    let site = QuickeningSite::<2>::new(Opcode::SetN);
    let values = PatchValues::from_site(&site);
    let key = crate::stencil_select::store_property_region_key();
    let view = crate::stencil_select::select_physical_for_abi(
        key,
        crate::stencil_select::RegionAbi::PropertyWriteGuard,
    )
    .expect("property-write view");
    let address = arena
        .render_physical_view_or_get(&mut cache, view, &values)
        .unwrap();
    arena.make_executable().unwrap();
    assert!(arena.property_guard_entry(address).is_err());
    let object =
        crate::value::ObjectData::new(vec![("value".into(), crate::value::Value::Number(1.0))]);
    let access = object
        .guarded_plain_slot(object.semantic_layout_id(), 0, "value")
        .expect("plain slot");
    let bits = crate::tagged_value::TaggedValue::number(7.5).bits();
    let mut context = crate::native_property::NativePropertyWriteContext::new(access, bits);
    let entry = arena
        .property_write_guard_entry(address)
        .expect("typed write entry");
    assert_eq!(entry(&mut context), 1);
    assert_eq!(
        object.hot_properties().slot_word(0).unwrap().load(),
        crate::value::Value::Number(7.5)
    );
}
