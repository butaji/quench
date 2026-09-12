/// AArch64 `ADD (immediate)` carries one unshifted twelve-bit byte offset.
pub const AARCH64_ADD_IMMEDIATE_FIELD_BITS: u32 = 12;
pub const AARCH64_ADD_IMMEDIATE_MAX: usize =
    (1_usize << AARCH64_ADD_IMMEDIATE_FIELD_BITS) - 1;

pub const AUDIT_SITE_BYTE_OFFSET_DELTA: usize = 16;
#[cfg(quench_stencil_audit_variant_c)]
const ACTIVE_SITE_BYTE_OFFSET_DELTA: usize = AUDIT_SITE_BYTE_OFFSET_DELTA;
#[cfg(not(quench_stencil_audit_variant_c))]
const ACTIVE_SITE_BYTE_OFFSET_DELTA: usize = 0;

/// Deliberately near the top of the encodable range so ordinary pointer
/// arithmetic cannot be mistaken for the copy-patch site-advance hole.
pub const NEXT_SITE_BYTE_OFFSET_HOLE: usize =
    AARCH64_ADD_IMMEDIATE_MAX - 7 - ACTIVE_SITE_BYTE_OFFSET_DELTA;
