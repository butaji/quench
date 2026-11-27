use super::*;

#[test]
fn function_body_admission_is_not_identity_or_static_knowledge() {
    let body = Type::function_body(7);
    let other = Type::function_body(9);
    let function = Type::language(ValueType::Function);
    assert_eq!(body.body_index(), Some(7));
    assert_eq!(body.join(body), body);
    assert_eq!(body.join(other), function);
    assert_eq!(body.join(Type::function(Value::direct_function(7)).unwrap()), function);
    assert!(body.is_subtype(function));
    assert!(!function.is_subtype(body));
    assert!(!body.is_subtype(other));
    let fact = Fact { ty: body, representation: Representation::Tagged,
        binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact };
    assert!(fact.verify());
    assert!(!Fact { proof: Proof::CanonicalValue, ..fact }.verify());
    assert!(!Fact { binding: BindingTime::Static, proof: Proof::Static, ..fact }.verify());
}

#[test]
fn function_identities_join_without_conflating_closures_or_static_proofs() {
    let first = Type::function(Value::direct_function(7)).unwrap();
    let second = Type::function(Value::direct_function(9)).unwrap();
    let function = Type::language(ValueType::Function);
    assert_eq!(first.function_identity(), Some(Value::direct_function(7)));
    assert_eq!(first.join(first), first);
    assert_eq!(first.join(second), function);
    assert_eq!(first.join(function), function);
    assert!(first.is_subtype(function));
    assert!(!function.is_subtype(first));
    assert!(!first.is_subtype(second));
    let fact = Fact { ty: first, representation: Representation::Tagged,
        binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact };
    assert!(fact.verify());
    assert!(!Fact { binding: BindingTime::Static, proof: Proof::Static, ..fact }.verify());
    assert!(Type::function(Value::integer(7)).is_none());
    assert_eq!(Type::immediate(Value::direct_function(7)), Type::ANY,
        "tag bits and source annotations cannot manufacture a function proof");
}


#[test]
fn joins_do_not_turn_bigint_or_unknown_into_number() {
    let bigint = Type::language(ValueType::BigInt);
    assert!(!Type::INT32.join(bigint).is_subtype(Type::NUMBER));
    assert!(!Type::NUMBER.join(Type::ANY).is_subtype(Type::NUMBER));
    assert_eq!(Type::INT32.join(Type::NUMBER), Type::NUMBER);
    assert_eq!(Type::NEVER.join(Type::INT32), Type::INT32);
}

#[test]
fn object_shapes_survive_only_compatible_joins_and_proof_lifetimes() {
    let object = Type::language(ValueType::Object);
    let shape = Type::direct_object(3);
    assert_eq!(shape.join(shape), shape);
    assert_eq!(shape.join(Type::NEVER), shape);
    assert_eq!(shape.join(Type::direct_object(4)), Type::DIRECT_OBJECT_FAMILY);
    assert_eq!(shape.join(object), object);
    assert!(shape.is_subtype(object));
    assert!(!object.is_subtype(shape));
    assert!(!Type::direct_object(4).is_subtype(shape));
    assert_eq!(shape.join(Type::language(ValueType::Null)).object_shape(), None);
    let fact = Fact { ty: shape, representation: Representation::Tagged,
        binding: BindingTime::Dynamic, proof: Proof::Definition, projection: Projection::Exact };
    assert!(fact.verified().is_some());
    assert!(Fact { binding: BindingTime::Location, proof: Proof::EntryGuard, ..fact }.verified().is_some());
    for (binding, proof) in [(BindingTime::Static, Proof::Static),
        (BindingTime::Location, Proof::CanonicalValue)] {
        assert!(Fact { binding, proof, ..fact }.verified().is_none());
    }
}

#[test]
fn nullable_shapes_preserve_only_guarded_region_facts() {
    let shape = Type::direct_object(3);
    let other = Type::direct_object(4);
    let null = Type::language(ValueType::Null);
    let undefined = Type::language(ValueType::Undefined);
    let nullable = shape.join(null);
    let nullish = nullable.join(undefined);
    assert_eq!(nullable.object_shape(), None, "null cannot authorize a record load");
    assert!(!nullable.is_direct_object_family());
    assert_eq!(nullable.direct_object_part(), Some(shape));
    assert_eq!(nullish.direct_object_part(), Some(shape));
    assert_eq!(nullable.join(shape), nullable);
    assert_eq!(nullable.join(shape.join(undefined)), nullish);
    assert_eq!(nullable.join(other), Type::DIRECT_OBJECT_FAMILY.join(null));
    assert!(null.is_subtype(nullable) && shape.is_subtype(nullable));
    assert!(!undefined.is_subtype(nullable) && !other.is_subtype(nullable));
    assert!(nullable.is_subtype(nullish));
    assert!(!Type::DIRECT_OBJECT_FAMILY.is_subtype(nullable));
    assert_eq!(nullable.without_object_shape(), Type::DIRECT_OBJECT_FAMILY.join(null));
    assert_eq!(SlotType::from_type(nullable), SlotType::DirectObjectOrNull);
    let fact = Fact { ty: nullable, representation: Representation::Tagged,
        binding: BindingTime::Dynamic, proof: Proof::Definition, projection: Projection::Exact };
    assert!(fact.verify());
    assert!(!Fact { binding: BindingTime::Static, proof: Proof::Static, ..fact }.verify());
    assert!(!Fact { binding: BindingTime::Site, proof: Proof::CanonicalValue, ..fact }.verify());
    let types = [Type::NEVER, shape, other, nullable, nullish, other.join(null),
        Type::DIRECT_OBJECT_FAMILY, Type::DIRECT_OBJECT_FAMILY.join(null), null, undefined,
        Type::NULLISH, Type::INT32, Type::NUMBER, Type::language(ValueType::Object), Type::ANY];
    for a in types { for b in types {
        let join = a.join(b);
        assert_eq!(join, b.join(a));
        assert!(a.is_subtype(join) && b.is_subtype(join));
        for c in types {
            assert_eq!(a.join(b).join(c), a.join(b.join(c)), "{a:?}, {b:?}, {c:?}");
            if a.is_subtype(c) && b.is_subtype(c) { assert!(join.is_subtype(c)); }
        }
    }}
}

#[test]
fn direct_object_family_is_weaker_than_shape_and_stronger_than_generic_object() {
    let family = Type::DIRECT_OBJECT_FAMILY;
    let object = Type::language(ValueType::Object);
    let shape = Type::direct_object(3);
    for direct in [family, shape] {
        assert!(direct.is_direct_object_family());
        assert!(direct.is_subtype(family));
        assert!(direct.is_subtype(object));
        assert_eq!(direct.join(family), family);
        assert_eq!(family.join(direct), family);
        assert_eq!(direct.join(object), object);
    }
    assert_eq!(family.object_shape(), None);
    assert!(!family.is_subtype(shape));
    for wider in [object, Type::ANY, object.join(Type::language(ValueType::Null))] {
        assert!(!wider.is_direct_object_family());
        assert!(!wider.is_subtype(family));
        assert_eq!(family.join(wider), wider);
    }
    let fact = Fact { ty: family, representation: Representation::Tagged,
        binding: BindingTime::Dynamic, proof: Proof::Definition, projection: Projection::Exact };
    assert!(fact.verified().is_some());
    assert!(Fact { binding: BindingTime::Location, proof: Proof::EntryGuard, ..fact }.verified().is_some());
    assert!(Fact { binding: BindingTime::Location, proof: Proof::CanonicalValue, ..fact }.verified().is_none());
    assert!(Fact { representation: Representation::Int32, ..fact }.verified().is_none());
}

#[test]
fn int32_representation_needs_a_range_proof() {
    for value in [-0.0, f64::NAN, f64::INFINITY, 2147483648.0, 0.5] {
        assert!(!Representation::Int32.accepts(Type::immediate(Value::number(value))));
        assert!(Representation::Float64.accepts(Type::immediate(Value::number(value))));
    }
    assert!(Representation::Tagged.accepts(Type::INT32));
    assert!(!Representation::Float64.accepts(Type::ANY));
    assert!(!Representation::Int32.accepts(Type::NEVER));
}

#[test]
fn a_fixed_location_or_observed_site_does_not_prove_its_value() {
    for binding in [BindingTime::Location, BindingTime::Site] {
        assert!(!Fact::number(Representation::Float64, binding, Proof::Static).verify());
        assert!(!Fact::number(Representation::Float64, binding, Proof::Definition).verify());
        assert!(Fact::number(Representation::Float64, binding, Proof::EntryGuard).verify());
    }
}

#[test]
fn selecting_an_int32_projection_does_not_prove_an_int32_result() {
    let fact = Fact::number(Representation::Int32, BindingTime::Dynamic, Proof::Definition);
    assert_eq!(fact.ty, Type::NUMBER);
    assert_eq!(fact.projection, Projection::CheckedDefinitions);
    assert!(!Representation::Int32.accepts(fact.ty));
    assert!(!Representation::Int32.can_receive(Representation::Float64));
    assert!(!Representation::Float64.can_receive(Representation::Tagged));
    assert!(Representation::Float64.can_receive(Representation::Int32));
}

#[test]
fn checked_projection_admission_requires_a_numeric_int32_destination() {
    let definition = Fact::number(Representation::Int32, BindingTime::Dynamic, Proof::Definition);
    assert!(definition.verified().is_some());
    for representation in [Representation::Float64, Representation::Tagged] {
        assert!(Fact { representation, ..definition }.verified().is_none());
    }
    for ty in [Type::NEVER, Type::ANY, Type::language(ValueType::Boolean),
        Type::NUMBER.join(Type::language(ValueType::BigInt))] {
        assert!(Fact { ty, ..definition }.verified().is_none());
    }
    assert!(Fact { ty: Type::INT32, ..definition }.verified().is_some());
}

#[test]
fn checked_definitions_cannot_stand_in_for_static_or_entry_proofs() {
    let definition = Fact::number(Representation::Int32, BindingTime::Dynamic, Proof::Definition);
    for (binding, proof) in [
        (BindingTime::Static, Proof::Static),
        (BindingTime::Location, Proof::EntryGuard),
        (BindingTime::Site, Proof::EntryGuard),
    ] {
        let fact = Fact { binding, proof, ..definition };
        assert!(fact.verified().is_none(), "{fact:?}");
    }
    assert!(definition.verified().is_some());
    assert!(Fact::number(Representation::Int32, BindingTime::Location, Proof::EntryGuard)
        .verified().is_some());
}

#[test]
fn nullable_direct_object_observations_do_not_authorize_record_access() {
    let family = Type::DIRECT_OBJECT_FAMILY;
    let null = Type::language(ValueType::Null);
    let undefined = Type::language(ValueType::Undefined);
    let nullable = family.join(null);
    let nullish = family.join(Type::NULLISH);
    assert_eq!(Type::immediate(Value::direct_object(7)), family);
    assert_eq!(nullable.join(undefined), nullish);
    assert_ne!(nullable, Type::language(ValueType::Object).join(null));
    assert!(!nullable.is_direct_object_family());
    assert!(nullable.object_shape().is_none());
    assert!(!undefined.is_subtype(nullable));
    for ty in [family, null, undefined] { assert!(ty.is_subtype(nullish)); }
    for value in [Value::direct_array(7), Value::direct_function(7), Value::integer(7), Value::DELETED] {
        assert!(!Type::immediate(value).is_subtype(nullish));
    }
    let observed = Fact { ty: nullish, representation: Representation::Tagged,
        binding: BindingTime::Site, proof: Proof::CanonicalValue, projection: Projection::Exact };
    assert!(observed.verified().is_none());
    assert!(Fact { proof: Proof::EntryGuard, ..observed }.verified().is_some());
    assert!(Fact { representation: Representation::Int32, proof: Proof::EntryGuard, ..observed }.verified().is_none());
}

#[test]
fn joins_preserve_each_semantic_input_and_never_invent_a_numeric_proof() {
    let types = [Type::NEVER, Type::INT32, Type::NUMBER, Type::ANY, Type::TYPEOF_INDEX,
        Type::DIRECT_OBJECT_FAMILY, Type::direct_object(3), Type::direct_object(4),
        Type::NULLISH, Type::DIRECT_OBJECT_FAMILY.join(Type::language(ValueType::Null)),
        Type::DIRECT_OBJECT_FAMILY.join(Type::language(ValueType::Undefined)),
        Type::DIRECT_OBJECT_FAMILY.join(Type::NULLISH),
        Type::language(ValueType::Undefined), Type::language(ValueType::Null),
        Type::language(ValueType::Boolean), Type::language(ValueType::String),
        Type::language(ValueType::Symbol), Type::language(ValueType::BigInt),
        Type::language(ValueType::Object), Type::language(ValueType::Function)];
    for left in types {
        for right in types {
            let joined = left.join(right);
            assert_eq!(joined, right.join(left));
            for third in types {
                assert_eq!(joined.join(third), left.join(right.join(third)),
                    "join associativity: {left:?}, {right:?}, {third:?}");
            }
            assert!(left.is_subtype(joined) && right.is_subtype(joined));
            for expected in [Type::INT32, Type::NUMBER, Type::language(ValueType::Object),
                Type::DIRECT_OBJECT_FAMILY, Type::direct_object(3), Type::NULLISH,
                Type::DIRECT_OBJECT_FAMILY.join(Type::NULLISH)] {
                assert_eq!(joined.is_subtype(expected),
                    left.is_subtype(expected) && right.is_subtype(expected));
            }
        }
    }
}

#[test]
fn admission_rejects_missing_types_and_unproved_machine_views() {
    let canonical = Fact::tagged(BindingTime::Location, Proof::CanonicalValue);
    for malformed in [
        Fact { ty: Type::NEVER, ..canonical },
        Fact { representation: Representation::Float64, ..canonical },
        Fact { ty: Type::NUMBER, ..canonical },
        Fact { proof: Proof::Static, ..canonical },
        Fact { binding: BindingTime::Static, ..canonical },
    ] {
        assert!(malformed.verified().is_none(), "{malformed:?}");
    }
    let union = Type::NUMBER.join(Type::language(ValueType::BigInt));
    let fact = Fact { ty: union, binding: BindingTime::Dynamic, proof: Proof::Definition, ..canonical };
    assert_eq!(fact.verified().unwrap().ty, union);
    assert!(Fact { representation: Representation::Float64, ..fact }.verified().is_none());
    assert_eq!(canonical.verified().unwrap().ty, Type::ANY);
}

fn integer_types() -> Vec<Type> {
    let ranges = [(0, 0), (0, 255), (-128, 127), (-1, 1), (5, 9), (0, i64::from(u32::MAX)), (i64::from(i32::MIN), i64::from(i32::MAX))];
    ranges.into_iter().map(|(min, max)| Type::int(IntRange { min, max })).collect()
}

#[test]
fn integer_ranges_are_number_subtypes_and_int32_is_the_widest_machine_range() {
    let wide = Type::int(IntRange { min: 0, max: i64::from(u32::MAX) });
    assert_eq!(Type::INT32, Type::int(IntRange::FULL));
    assert!(Type::int(IntRange { min: 0, max: 255 }).is_subtype(Type::INT32));
    assert!(Type::INT32.is_subtype(Type::NUMBER) && wide.is_subtype(Type::NUMBER));
    assert!(!wide.is_subtype(Type::INT32) && !Type::NUMBER.is_subtype(Type::INT32));
    assert!(Type::int(IntRange::singleton(3)).is_int32() && !wide.is_int32() && !Type::NUMBER.is_int32());
    assert_eq!(Type::int(IntRange::singleton(3)).machine(), Type::INT32);
    assert_eq!(wide.machine(), Type::NUMBER);
    assert_eq!(Type::ANY.machine(), Type::ANY);
    assert_eq!(SlotType::from_type(Type::int(IntRange { min: 0, max: 255 })), SlotType::Int32);
    assert_eq!(SlotType::from_type(wide), SlotType::Number);
    let hull = Type::int(IntRange { min: 0, max: 9 }).join(Type::int(IntRange { min: 100, max: 200 }));
    assert_eq!(hull, Type::int(IntRange { min: 0, max: 200 }));
    assert_eq!(hull.join(Type::NUMBER), Type::NUMBER);
    assert!(!hull.join(Type::language(ValueType::String)).is_subtype(Type::NUMBER));
}

#[test]
fn integer_joins_are_commutative_idempotent_associative_and_monotone() {
    let mut types = integer_types();
    types.extend([Type::NEVER, Type::NUMBER, Type::ANY, Type::language(ValueType::String), Type::direct_object(3)]);
    for a in &types { for b in &types {
        assert_eq!(a.join(*b), b.join(*a));
        assert_eq!(a.join(*a), *a);
        assert!(a.is_subtype(a.join(*b)) && b.is_subtype(a.join(*b)));
        for c in &types {
            assert_eq!(a.join(*b).join(*c), a.join(b.join(*c)), "{a:?}, {b:?}, {c:?}");
            if a.is_subtype(*b) { assert!(a.join(*c).is_subtype(b.join(*c)), "{a:?} <= {b:?}, {c:?}"); }
        }
    }}
}

#[test]
fn widening_contains_the_join_and_every_chain_of_widenings_terminates() {
    let mut types = integer_types();
    types.extend([Type::NEVER, Type::NUMBER, Type::language(ValueType::String)]);
    for start in &types { for next in &types {
        assert!(start.join(*next).is_subtype(start.widen(*next)), "{start:?} widen {next:?}");
        let mut settled = *start;
        let mut steps = 0;
        loop {
            let widened = settled.widen(*next);
            if widened == settled { break; }
            settled = widened;
            steps += 1;
            assert!(steps <= 3, "a widening chain of {start:?} by {next:?} does not settle");
        }
    }}
    let counter = (1..=i64::from(WIDENING_ROUNDS) * 4).fold(Type::int(IntRange::singleton(0)),
        |known, step| known.widen(Type::int(IntRange { min: 0, max: step })));
    assert_eq!(counter, Type::int(IntRange::NON_NEGATIVE));
}

#[test]
fn a_64_bit_integer_view_holds_the_integers_a_number_holds_exactly() {
    let exact = Type::int(IntRange::EXACT);
    assert!(Representation::Int64.accepts(exact));
    assert!(Representation::Int64.accepts(Type::INT32));
    assert!(!Representation::Int64.accepts(Type::int(IntRange::WORD)));
    assert!(!Representation::Int64.accepts(Type::NUMBER));
    assert!(!Representation::Int64.accepts(Type::NEVER));
    assert!(Representation::Float64.accepts(Type::int(IntRange::WORD)) && !Representation::Int32.accepts(exact));
    assert!(Representation::Int64.can_receive(Representation::Int32) && !Representation::Int32.can_receive(Representation::Int64));
    assert!(Representation::Float64.can_receive(Representation::Int64) && !Representation::Int64.can_receive(Representation::Float64));
    let held = Fact { ty: exact, representation: Representation::Int64, binding: BindingTime::Dynamic,
        proof: Proof::Definition, projection: Projection::Exact };
    assert!(held.verified().is_some());
    assert!(Fact { ty: Type::int(IntRange::WORD), ..held }.verified().is_none());
}

#[test]
fn a_types_representation_is_the_narrowest_view_that_accepts_it() {
    use super::representation::repr;
    let exact = Type::int(IntRange::EXACT);
    let wide = Type::int(IntRange::WORD);
    let cases = [
        (Type::INT32, Representation::Int32),
        (Type::int(IntRange::singleton(7)), Representation::Int32),
        (exact, Representation::Int64),
        (Type::int(IntRange { min: i64::from(i32::MAX), max: 1 << 40 }), Representation::Int64),
        (wide, Representation::Float64),
        (Type::NUMBER, Representation::Float64),
        (Type::NUMBER.join(Type::INT32), Representation::Float64),
        (Type::INT32.join(Type::language(ValueType::BigInt)), Representation::Tagged),
        (Type::ANY, Representation::Tagged),
        (Type::NEVER, Representation::Tagged),
    ];
    for (ty, expected) in cases {
        assert_eq!(repr(ty), expected, "{ty:?}");
    }
    assert_eq!(repr(Type::NUMBER.join(Type::ANY)), Representation::Tagged);
}

#[test]
fn string_atoms_join_to_a_bounded_set_then_any_string_of_the_hull() {
    let a = Type::string_atoms(&[5], 3, 3);
    let b = Type::string_atoms(&[2], 1, 4);
    let joined = a.join(b);
    assert_eq!(joined.string_fact().unwrap().atoms(), &[2, 5]);
    assert_eq!(joined.string_fact().unwrap().length(), StringLength { min: 1, max: 4 });
    assert!(a.is_subtype(joined) && !joined.is_subtype(a));
    assert!(a.is_subtype(Type::language(ValueType::String)));
    let many = (0..=MAX_STRING_ATOMS as u32).fold(Type::NEVER, |ty, atom| ty.join(Type::string_atoms(&[atom], 1, 1)));
    assert_eq!(many.string_fact().unwrap().atoms(), &[] as &[u32]);
    assert_eq!(many.string_fact().unwrap().length(), StringLength { min: 1, max: 1 });
}

#[test]
fn full_length_string_without_atoms_is_the_plain_string_type() {
    assert_eq!(Type::string_length(0, u32::MAX), Type::language(ValueType::String));
    assert_eq!(Type::language(ValueType::String).string_fact(), Some(StringFact::ANY));
    assert_eq!(Type::string_length(1, 1).join(Type::language(ValueType::String)), Type::language(ValueType::String));
    assert_eq!(Type::string_length(1, 1).join(Type::NUMBER).string_fact(), None);
}

#[test]
fn string_widening_reaches_a_fixed_length_range() {
    let grown = Type::string_length(1, 1).widen(Type::string_length(1, 2));
    assert_eq!(grown.string_fact().unwrap().length(), StringLength { min: 1, max: u32::MAX });
}
