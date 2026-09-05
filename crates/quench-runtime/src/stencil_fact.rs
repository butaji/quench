//! Canonical, allocation-free facts shared by region stencils.
//!
//! This module describes data.  It does not select, copy, patch, or execute
//! machine code; those effects live at the explicit edges of the stencil tier.

use crate::dynamic::{JsValue, Tag};
use crate::quickening::QuickeningSite;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct RegionId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct RegionKey(pub u64);

/// A compact certainty entry.  The payload is a canonical runtime-independent
/// fact code; runtime shape/callee identities remain in PatchValues.
/// Stencils reuse the operation fact certainty vocabulary; this alias is not
/// a second classification or storage representation.
pub use crate::facts::Certainty as FactState;

impl RegionKey {
    /// Stable FNV-1a based key.  It intentionally does not use
    /// DefaultHasher, whose seed is process-specific.
    pub const fn from_facts(region: RegionId, facts: &[FactState]) -> Self {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        hash = mix(hash, region.0);
        hash = mix(hash, facts.len() as u32);
        let mut index = 0;
        while index < facts.len() {
            hash = mix(hash, facts[index] as u32);
            index += 1;
        }
        Self(hash)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    /// Derive a region key directly from the canonical opcode catalog.  The
    /// region layer does not maintain a parallel eligibility declaration.
    pub const fn from_opcodes(region: RegionId, opcodes: &[crate::ir::Opcode]) -> Self {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        hash = mix(hash, region.0);
        hash = mix(hash, opcodes.len() as u32);
        let mut index = 0;
        while index < opcodes.len() {
            // Region identity includes the canonical operation sequence, not
            // only its certainty states.  Otherwise two distinct opcode
            // sequences sharing a region id and guard profile could select
            // the wrong stencil.
            hash = mix(hash, opcodes[index] as u32);
            hash = mix(hash, opcodes[index].stencil_certainty() as u32);
            index += 1;
        }
        Self(hash)
    }
}

const fn mix(mut hash: u64, value: u32) -> u64 {
    let bytes = value.to_le_bytes();
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
        index += 1;
    }
    hash
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum HoleKind {
    Imm32,
    Disp32,
    Rel32,
    /// AArch64 `B` immediate: signed word displacement in bits [25:0].
    Branch26,
    Ptr64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Hole {
    pub offset: u16,
    pub kind: HoleKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stencil {
    pub bytes: &'static [u8],
    pub holes: &'static [Hole],
}

impl Stencil {
    pub fn validate(&self) -> bool {
        self.holes.iter().enumerate().all(|(index, hole)| {
            let width = match hole.kind {
                HoleKind::Imm32 | HoleKind::Disp32 | HoleKind::Rel32 | HoleKind::Branch26 => 4,
                HoleKind::Ptr64 => 8,
            };
            let aligned = !matches!(hole.kind, HoleKind::Branch26)
                || usize::from(hole.offset) % 4 == 0;
            let disjoint = self.holes[..index].iter().all(|prior| {
                let prior_width = match prior.kind {
                    HoleKind::Imm32
                    | HoleKind::Disp32
                    | HoleKind::Rel32
                    | HoleKind::Branch26 => 4,
                    HoleKind::Ptr64 => 8,
                };
                let start = usize::from(hole.offset);
                let prior_start = usize::from(prior.offset);
                start.saturating_add(width) <= prior_start
                    || prior_start.saturating_add(prior_width) <= start
            });
            aligned && disjoint && usize::from(hole.offset).saturating_add(width) <= self.bytes.len()
        })
    }
}

/// A read-only view over the existing quickening state.  It deliberately
/// borrows the site instead of copying cache entries into a second cache.
#[derive(Clone, Copy)]
pub struct PatchValues<'a, const N: usize = 4> {
    site: &'a QuickeningSite<N>,
    relative_target: Option<(usize, usize)>,
    constant_bits: Option<u64>,
    /// A pointer-sized relocation used by generated entry trampolines.  It is
    /// kept separate from numeric constant bits so a machine-code entry can
    /// never accidentally interpret a JavaScript number as a code address.
    pointer_bits: Option<u64>,
}

impl<'a, const N: usize> PatchValues<'a, N> {
    pub fn from_site(site: &'a QuickeningSite<N>) -> Self {
        Self {
            site,
            relative_target: None,
            constant_bits: None,
            pointer_bits: None,
        }
    }

    /// Attach a build-time-known numeric constant to the rendered leaf. The
    /// value is patch data, not a semantic opcode or a second constant pool.
    pub fn with_constant_bits(mut self, bits: u64) -> Self {
        self.constant_bits = Some(bits);
        self
    }

    /// Attach a code/data pointer relocation to a generated stencil.  The
    /// pointer is patch data only; semantic values remain owned by the Rust
    /// handler reached by the trampoline.
    pub fn with_pointer_bits(mut self, pointer: usize) -> Self {
        self.pointer_bits = Some(pointer as u64);
        self
    }

    /// Attach a build-selected relative branch target without copying any
    /// quickening state. For `Rel32`, `next_instruction` is the address after
    /// the four-byte displacement; the AArch64 `Branch26` writer instead
    /// supplies the branch instruction's own PC, as required by `B`.
    pub fn with_relative_target(self, target: usize, next_instruction: usize) -> Option<Self> {
        let displacement = target as i128 - next_instruction as i128;
        (i128::from(i32::MIN)..=i128::from(i32::MAX))
            .contains(&displacement)
            .then_some(Self {
                relative_target: Some((target, next_instruction)),
                ..self
            })
    }

    pub fn opcode(&self) -> crate::ir::Opcode {
        self.site.opcode()
    }

    pub fn certainty(&self) -> crate::facts::Certainty {
        self.site.certainty()
    }

    pub fn misses(&self) -> u8 {
        self.site.misses()
    }

    pub fn shape_count(&self) -> usize {
        self.site.cache_len()
    }

    pub fn callable_count(&self) -> usize {
        self.site.callable_cache_len()
    }

    /// Stable cache identity for the disposable rendered machine-code view.
    ///
    /// A region key describes semantic selection, but the bytes also contain
    /// patchable quickening facts. Keeping those facts out of the semantic
    /// key is intentional; they still must participate in memoization or a
    /// later invocation could execute code patched for an earlier cache
    /// state. Relative targets contribute their displacement because that is
    /// the actual value written into the branch instruction.
    pub(crate) fn signature(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        hash = mix_u64(hash, self.site.patch_signature());
        let target_displacement = self
            .relative_target
            .map_or(0, |(target, next)| target.wrapping_sub(next) as u64);
        hash = mix_u64(hash, target_displacement);
        match self.constant_bits {
            Some(bits) => {
                hash = mix_u64(hash, 1);
                hash = mix_u64(hash, bits);
            }
            None => hash = mix_u64(hash, 0),
        }
        match self.pointer_bits {
            Some(pointer) => {
                hash = mix_u64(hash, 1);
                mix_u64(hash, pointer)
            }
            None => mix_u64(hash, 0),
        }
    }

    pub(crate) fn value_for(&self, kind: HoleKind) -> u64 {
        match kind {
            HoleKind::Imm32 => u64::from(self.misses()),
            HoleKind::Disp32 => self.shape_count() as u64,
            HoleKind::Rel32 => self
                .relative_target
                .map_or(self.callable_count() as u64, |(target, next)| {
                    target.wrapping_sub(next) as u64
                }),
            HoleKind::Branch26 => self
                .relative_target
                .map_or(0, |(target, next)| target.wrapping_sub(next) as u64),
            HoleKind::Ptr64 => self
                .pointer_bits
                .or(self.constant_bits)
                .unwrap_or(self.opcode() as u64),
        }
    }
}

fn mix_u64(mut hash: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

impl std::fmt::Debug for PatchValues<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PatchValues")
            .field("opcode", &self.opcode())
            .field("certainty", &self.certainty())
            .field("misses", &self.misses())
            .field("shape_count", &self.shape_count())
            .field("callable_count", &self.callable_count())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BaseType {
    Number,
    Boolean,
    Object,
    String,
    BigInt,
    Nullish,
    Callable,
}

/// Data-level description of the one JsValue boxing scheme.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoxingFact {
    pub base_type: BaseType,
    pub tags: &'static [Tag],
}

macro_rules! boxing_catalog {
    ($(($name:ident, [$($tag:path),+])),+ $(,)?) => {
        impl BoxingFact {
            pub const fn for_type(base_type: BaseType) -> Self {
                match base_type {
                    $(BaseType::$name => Self { base_type, tags: &[$($tag),+] },)+
                }
            }

            pub const fn all() -> [Self; 7] {
                [$(Self::for_type(BaseType::$name)),+]
            }
        }
    };
}

// One tag-layout declaration derives both the per-type fact and the complete
// enumeration consumed by build-time specialization.
boxing_catalog!(
    (Number, [Tag::Int, Tag::Float64]),
    (Boolean, [Tag::Bool]),
    (Object, [Tag::Object]),
    (String, [Tag::String, Tag::StringRope]),
    (BigInt, [Tag::BigInt, Tag::ShortBigInt]),
    (Nullish, [Tag::Null, Tag::Undefined]),
    (Callable, [Tag::FunctionBytecode]),
);

impl BoxingFact {
    pub fn accepts(&self, value: &JsValue) -> bool {
        self.tags.iter().any(|tag| value.tag() == *tag)
    }

    pub fn predicate(&self, value: &JsValue) -> bool {
        self.accepts(value)
    }

    pub fn from_tag(tag: Tag) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|fact| fact.tags.iter().any(|candidate| *candidate == tag))
    }
}

/// A small stable digest for a fact vector, useful to build scripts that do
/// not want to depend on hash-map iteration order.
pub fn digest_facts(region: RegionId, facts: &[FactState]) -> RegionKey {
    RegionKey::from_facts(region, facts)
}

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
    }
}
