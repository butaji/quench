#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_fact_vectors_have_identical_keys() {
        let facts = [FactState::Proven, FactState::Guarded, FactState::Unknown];
        assert_eq!(
            RegionKey::from_facts(RegionId(3), &facts),
            RegionKey::from_facts(RegionId(3), &facts)
        );
        assert_ne!(
            RegionKey::from_facts(RegionId(3), &facts),
            RegionKey::from_facts(RegionId(4), &facts)
        );
        let opcodes = [crate::ir::Opcode::Add, crate::ir::Opcode::Return];
        assert_eq!(
            RegionKey::from_opcodes(RegionId(1), &opcodes),
            RegionKey::from_opcodes(RegionId(1), &opcodes)
        );
        assert_ne!(
            RegionKey::from_opcodes(RegionId(1), &opcodes),
            RegionKey::from_facts(
                RegionId(1),
                &[
                    crate::facts::Certainty::Guarded,
                    crate::facts::Certainty::Proven
                ]
            )
        );
        assert_ne!(
            RegionKey::from_opcodes(RegionId(2), &[crate::ir::Opcode::GetProperty]),
            RegionKey::from_facts(RegionId(2), &[FactState::Guarded])
        );
        assert_ne!(
            RegionKey::from_opcodes(
                RegionId(9),
                &[crate::ir::Opcode::Add, crate::ir::Opcode::Return]
            ),
            RegionKey::from_opcodes(
                RegionId(9),
                &[crate::ir::Opcode::Sub, crate::ir::Opcode::Return]
            )
        );
    }

    #[test]
    fn boxing_predicates_follow_jsvalue_tags() {
        let values = [
            (BaseType::Number, JsValue::Int(1)),
            (BaseType::Number, JsValue::Float64(-0.0)),
            (BaseType::Boolean, JsValue::Bool(true)),
            (BaseType::Object, JsValue::ptr(Tag::Object, 1)),
            (BaseType::String, JsValue::ptr(Tag::String, 1)),
            (BaseType::BigInt, JsValue::ShortBigInt(2)),
            (BaseType::Nullish, JsValue::Null),
            (BaseType::Nullish, JsValue::Undefined),
            (BaseType::Callable, JsValue::ptr(Tag::FunctionBytecode, 1)),
        ];
        for (kind, value) in values {
            assert!(BoxingFact::for_type(kind).accepts(&value));
        }
        assert!(!BoxingFact::for_type(BaseType::Boolean).accepts(&JsValue::Int(1)));

        // Exercise every tag in the fixed JsValue layout, including tags that
        // intentionally have no language-level base-type fact.
        let tags = [
            Tag::BigInt,
            Tag::Symbol,
            Tag::String,
            Tag::StringRope,
            Tag::Module,
            Tag::FunctionBytecode,
            Tag::Object,
            Tag::Int,
            Tag::Bool,
            Tag::Null,
            Tag::Undefined,
            Tag::Uninitialized,
            Tag::CatchOffset,
            Tag::Exception,
            Tag::ShortBigInt,
            Tag::Float64,
        ];
        for tag in tags {
            let value = JsValue::ptr(tag, 1);
            let matching_facts = BoxingFact::all()
                .into_iter()
                .filter(|fact| fact.accepts(&value))
                .count();
            assert_eq!(
                matching_facts,
                usize::from(BoxingFact::from_tag(tag).is_some())
            );
        }
    }

    #[test]
    fn stencil_validation_rejects_misaligned_or_overlapping_relocations() {
        static BYTES: [u8; 16] = [0; 16];
        assert!(!Stencil {
            bytes: &BYTES,
            holes: &[Hole {
                offset: 2,
                kind: HoleKind::Branch26,
            }],
        }
        .validate());
        assert!(!Stencil {
            bytes: &BYTES,
            holes: &[Hole {
                offset: 2,
                kind: HoleKind::CondBranch19,
            }],
        }
        .validate());
        assert!(!Stencil {
            bytes: &BYTES,
            holes: &[
                Hole {
                    offset: 0,
                    kind: HoleKind::Ptr64,
                },
                Hole {
                    offset: 4,
                    kind: HoleKind::Imm32,
                },
            ],
        }
        .validate());
        assert!(Stencil {
            bytes: &BYTES,
            holes: &[
                Hole {
                    offset: 0,
                    kind: HoleKind::Branch26,
                },
                Hole {
                    offset: 8,
                    kind: HoleKind::Ptr64,
                },
            ],
        }
        .validate());
    }

    #[test]
    fn patch_signature_tracks_relative_branch_displacement() {
        let site = QuickeningSite::<2>::new(crate::ir::Opcode::Add);
        let values = PatchValues::from_site(&site);
        let first = values
            .with_relative_target(0x1100, 0x1000)
            .expect("rel32 displacement");
        let second = values
            .with_relative_target(0x1200, 0x1000)
            .expect("rel32 displacement");
        assert_ne!(first.signature(), second.signature());

        let mut first_site = QuickeningSite::<2>::new(crate::ir::Opcode::GetProperty);
        let mut second_site = QuickeningSite::<2>::new(crate::ir::Opcode::GetProperty);
        assert!(matches!(
            first_site.observe(
                crate::shape_cache::ShapeId(1),
                crate::shape_cache::PropertyId(4),
                7
            ),
            crate::quickening::QuickeningDecision::InstallGuard { .. }
        ));
        assert!(matches!(
            second_site.observe(
                crate::shape_cache::ShapeId(2),
                crate::shape_cache::PropertyId(4),
                7
            ),
            crate::quickening::QuickeningDecision::InstallGuard { .. }
        ));
        assert_ne!(
            PatchValues::from_site(&first_site).signature(),
            PatchValues::from_site(&second_site).signature()
        );

        let pointer_a = PatchValues::from_site(&first_site).with_pointer_bits(0x1000);
        let pointer_b = PatchValues::from_site(&first_site).with_pointer_bits(0x2000);
        assert_ne!(pointer_a.signature(), pointer_b.signature());
        assert_eq!(pointer_a.value_for(HoleKind::Ptr64), 0x1000);
        let constant = PatchValues::from_site(&first_site).with_constant_bits(0x1234);
        assert_eq!(constant.value_for(HoleKind::Literal64), 0x1234);
        assert_eq!(
            constant.value_for(HoleKind::Ptr64),
            crate::ir::Opcode::GetProperty as u64
        );
    }
}
