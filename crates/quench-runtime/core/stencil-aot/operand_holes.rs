#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperandHoleKind {
    DestinationRegisterByteOffset,
    SourceRegisterByteOffset,
    LeftRegisterByteOffset,
    RightRegisterByteOffset,
    DestinationLocalByteOffset,
    SourceLocalByteOffset,
}

pub const VALUE_BYTE_WIDTH: usize = core::mem::size_of::<u64>();
pub const AARCH64_UNSIGNED_OFFSET_FIELD_BITS: u32 = 12;
pub const AARCH64_UNSIGNED_OFFSET_MAX_SCALED: usize =
    (1_usize << AARCH64_UNSIGNED_OFFSET_FIELD_BITS) - 1;

// The differential cooker audit compiles the complete catalog once with a
// second, still-encodable set of placeholders. Production builds use zero.
// Keep the delta larger than the number of distinct placeholder slots so the
// two sets cannot overlap.
pub const AUDIT_PLACEHOLDER_SLOT_DELTA: usize = 16;
#[cfg(quench_stencil_audit_variant_c)]
const ACTIVE_PLACEHOLDER_SLOT_DELTA: usize = AUDIT_PLACEHOLDER_SLOT_DELTA;
#[cfg(not(quench_stencil_audit_variant_c))]
const ACTIVE_PLACEHOLDER_SLOT_DELTA: usize = 0;

// These deliberately occupy the top encodable 64-bit load/store slots.
// The cooker recognizes them as typed holes and the linker replaces them with
// the derived byte offsets of real VM registers.
pub const DESTINATION_LOCAL_HOLE_SLOT: usize =
    AARCH64_UNSIGNED_OFFSET_MAX_SCALED - 4 - ACTIVE_PLACEHOLDER_SLOT_DELTA;
pub const SOURCE_LOCAL_HOLE_SLOT: usize =
    AARCH64_UNSIGNED_OFFSET_MAX_SCALED - 3 - ACTIVE_PLACEHOLDER_SLOT_DELTA;
pub const DESTINATION_REGISTER_HOLE_SLOT: usize =
    AARCH64_UNSIGNED_OFFSET_MAX_SCALED - 2 - ACTIVE_PLACEHOLDER_SLOT_DELTA;
pub const SOURCE_REGISTER_HOLE_SLOT: usize =
    AARCH64_UNSIGNED_OFFSET_MAX_SCALED - 5 - ACTIVE_PLACEHOLDER_SLOT_DELTA;
pub const LEFT_REGISTER_HOLE_SLOT: usize =
    AARCH64_UNSIGNED_OFFSET_MAX_SCALED - 1 - ACTIVE_PLACEHOLDER_SLOT_DELTA;
pub const RIGHT_REGISTER_HOLE_SLOT: usize =
    AARCH64_UNSIGNED_OFFSET_MAX_SCALED - ACTIVE_PLACEHOLDER_SLOT_DELTA;

pub const COPY_FROM_LOCAL_HOLES: &[OperandHoleKind] = &[
    OperandHoleKind::DestinationRegisterByteOffset,
    OperandHoleKind::SourceLocalByteOffset,
];
pub const COPY_TO_LOCAL_HOLES: &[OperandHoleKind] = &[
    OperandHoleKind::DestinationLocalByteOffset,
    OperandHoleKind::SourceRegisterByteOffset,
];
pub const REGISTER_COPY_HOLES: &[OperandHoleKind] = &[
    OperandHoleKind::DestinationRegisterByteOffset,
    OperandHoleKind::SourceRegisterByteOffset,
];
pub const BINARY_REGISTER_HOLES: &[OperandHoleKind] = &[
    OperandHoleKind::DestinationRegisterByteOffset,
    OperandHoleKind::LeftRegisterByteOffset,
    OperandHoleKind::RightRegisterByteOffset,
];

pub const fn placeholder_byte_offset(kind: OperandHoleKind) -> usize {
    placeholder_slot(kind) * VALUE_BYTE_WIDTH
}

pub const fn placeholder_slot(kind: OperandHoleKind) -> usize {
    match kind {
        OperandHoleKind::DestinationRegisterByteOffset => DESTINATION_REGISTER_HOLE_SLOT,
        OperandHoleKind::SourceRegisterByteOffset => SOURCE_REGISTER_HOLE_SLOT,
        OperandHoleKind::LeftRegisterByteOffset => LEFT_REGISTER_HOLE_SLOT,
        OperandHoleKind::RightRegisterByteOffset => RIGHT_REGISTER_HOLE_SLOT,
        OperandHoleKind::DestinationLocalByteOffset => DESTINATION_LOCAL_HOLE_SLOT,
        OperandHoleKind::SourceLocalByteOffset => SOURCE_LOCAL_HOLE_SLOT,
    }
}

pub const fn kind_for_placeholder_slot(slot: usize) -> Option<OperandHoleKind> {
    match slot {
        DESTINATION_REGISTER_HOLE_SLOT => Some(OperandHoleKind::DestinationRegisterByteOffset),
        SOURCE_REGISTER_HOLE_SLOT => Some(OperandHoleKind::SourceRegisterByteOffset),
        LEFT_REGISTER_HOLE_SLOT => Some(OperandHoleKind::LeftRegisterByteOffset),
        RIGHT_REGISTER_HOLE_SLOT => Some(OperandHoleKind::RightRegisterByteOffset),
        DESTINATION_LOCAL_HOLE_SLOT => Some(OperandHoleKind::DestinationLocalByteOffset),
        SOURCE_LOCAL_HOLE_SLOT => Some(OperandHoleKind::SourceLocalByteOffset),
        _ => None,
    }
}

pub fn expected_kinds_for_stencil(name: &str) -> Option<&'static [OperandHoleKind]> {
    match name {
        "quench_region_burned_load_literal_one_lane"
        | "quench_region_burned_load_literal_two_lanes"
        | "quench_region_burned_load_literal_three_lanes"
        | "quench_region_burned_load_literal_four_lanes" => {
            Some(&[OperandHoleKind::DestinationRegisterByteOffset])
        }
        "quench_region_burned_load_local" => Some(COPY_FROM_LOCAL_HOLES),
        "quench_region_burned_store_local" => Some(COPY_TO_LOCAL_HOLES),
        "quench_region_burned_move"
        | "quench_region_burned_unary_plus"
        | "quench_region_burned_negate"
        | "quench_region_burned_bit_not" => Some(REGISTER_COPY_HOLES),
        "quench_region_burned_add"
        | "quench_region_burned_subtract"
        | "quench_region_burned_multiply"
        | "quench_region_burned_divide"
        | "quench_region_burned_equal"
        | "quench_region_burned_not_equal"
        | "quench_region_burned_less"
        | "quench_region_burned_less_equal"
        | "quench_region_burned_greater"
        | "quench_region_burned_greater_equal" => Some(BINARY_REGISTER_HOLES),
        _ => None,
    }
}
