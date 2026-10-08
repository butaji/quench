pub(crate) const UTF16_MAX_CODE_UNIT: u32 = u16::MAX as u32;
pub(crate) const ASCII_CODE_UNIT_LIMIT: u16 = 0x80;
pub(crate) const HIGH_SURROGATE_START: u16 = 0xD800;
pub(crate) const HIGH_SURROGATE_END: u16 = 0xDBFF;
pub(crate) const LOW_SURROGATE_START: u16 = 0xDC00;
pub(crate) const LOW_SURROGATE_END: u16 = 0xDFFF;
pub(crate) const SURROGATE_START: u32 = HIGH_SURROGATE_START as u32;
pub(crate) const SURROGATE_END: u32 = LOW_SURROGATE_END as u32;
pub(crate) const UNICODE_MAX_CODE_POINT: u32 = 0x10_FFFF;
pub(crate) const SURROGATE_PAYLOAD_MASK: u32 = 0x03FF;
pub(crate) const SURROGATE_PAYLOAD_SHIFT: u32 = 10;
pub(crate) const SUPPLEMENTARY_CODE_POINT_START: u32 = 0x1_0000;

pub(crate) const fn is_high_surrogate(unit: u16) -> bool {
    (unit >= HIGH_SURROGATE_START) && (unit <= HIGH_SURROGATE_END)
}

pub(crate) const fn is_low_surrogate(unit: u16) -> bool {
    (unit >= LOW_SURROGATE_START) && (unit <= LOW_SURROGATE_END)
}

pub(crate) const fn is_surrogate(code_point: u32) -> bool {
    (code_point >= SURROGATE_START) && (code_point <= SURROGATE_END)
}

pub(crate) const fn decode_surrogate_pair(high: u16, low: u16) -> Option<u32> {
    if !is_high_surrogate(high) || !is_low_surrogate(low) {
        return None;
    }
    let high_payload = (high as u32) - SURROGATE_START;
    let low_payload = (low as u32) - LOW_SURROGATE_START as u32;
    Some(
        SUPPLEMENTARY_CODE_POINT_START
            + (high_payload << SURROGATE_PAYLOAD_SHIFT)
            + (low_payload & SURROGATE_PAYLOAD_MASK),
    )
}
