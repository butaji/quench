pub const RAW_VALUE_HOLE_ID: u8 = 0;
pub const OBSERVATION_COUNTER_HOLE_ID: u8 = 1;
pub const WORD32_LITERAL_HOLE_ID: u8 = 2;
pub const LOWEST_RAW_VALUE_LANE: u32 = 0;
pub const SECOND_RAW_VALUE_LANE: u32 = 1;
pub const THIRD_RAW_VALUE_LANE: u32 = 2;
pub const HIGHEST_RAW_VALUE_LANE: u32 = 3;
pub const RAW_VALUE_LANE_BITS: u32 = 16;
pub const RAW_VALUE_LANE_COUNT: usize = u64::BITS as usize / RAW_VALUE_LANE_BITS as usize;
pub const ONE_HIGH_LANE_MASK: u8 = 1 << HIGHEST_RAW_VALUE_LANE;
pub const TWO_HIGH_LANES_MASK: u8 = ONE_HIGH_LANE_MASK | (1 << THIRD_RAW_VALUE_LANE);
pub const THREE_HIGH_LANES_MASK: u8 = TWO_HIGH_LANES_MASK | (1 << SECOND_RAW_VALUE_LANE);
pub const ALL_RAW_VALUE_LANES_MASK: u8 = THREE_HIGH_LANES_MASK | (1 << LOWEST_RAW_VALUE_LANE);
pub const LOW_TWO_LANES_MASK: u8 =
    (1 << LOWEST_RAW_VALUE_LANE) | (1 << SECOND_RAW_VALUE_LANE);
pub const LOWEST_LANE_BITS_MASK: u64 = u16::MAX as u64;
pub const LOW_TWO_LANES_BITS_MASK: u64 = u32::MAX as u64;
pub const LOW_THREE_LANES_BITS_MASK: u64 = (1_u64 << (RAW_VALUE_LANE_BITS * 3)) - 1;

// Every sixteen-bit lane is nonzero, non-0xffff, and distinct so rustc emits
// one MOVZ plus three MOVK instructions. The audit value changes every lane;
// the cooker may therefore prove that only declared immediate fields vary.
pub const RAW_VALUE_HOLE_BITS: u64 = if cfg!(quench_stencil_audit_variant_c) {
    0x2468_ace0_1357_9bdf
} else {
    0x1357_9bdf_2468_ace0
};

pub const ONE_HIGH_LANE_HOLE_BITS: u64 = RAW_VALUE_HOLE_BITS & !LOW_THREE_LANES_BITS_MASK;
pub const TWO_HIGH_LANES_HOLE_BITS: u64 = RAW_VALUE_HOLE_BITS & !LOW_TWO_LANES_BITS_MASK;
pub const THREE_HIGH_LANES_HOLE_BITS: u64 = RAW_VALUE_HOLE_BITS & !LOWEST_LANE_BITS_MASK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawValueHoleSpec {
    pub id: u8,
    pub lane_mask: u8,
}

pub fn expected_raw_value_hole(name: &str) -> Option<RawValueHoleSpec> {
    if name.starts_with("quench_register_region_load_word_literal_w") {
        return Some(RawValueHoleSpec {
            id: WORD32_LITERAL_HOLE_ID,
            lane_mask: LOW_TWO_LANES_MASK,
        });
    }
    let (id, lane_mask) = match name {
        "quench_region_burned_load_literal_one_lane" => (RAW_VALUE_HOLE_ID, ONE_HIGH_LANE_MASK),
        "quench_region_burned_load_literal_two_lanes" => (RAW_VALUE_HOLE_ID, TWO_HIGH_LANES_MASK),
        "quench_region_burned_load_literal_three_lanes" => {
            (RAW_VALUE_HOLE_ID, THREE_HIGH_LANES_MASK)
        }
        "quench_region_burned_load_literal_four_lanes" => {
            (RAW_VALUE_HOLE_ID, ALL_RAW_VALUE_LANES_MASK)
        }
        "quench_observe_entry" => (OBSERVATION_COUNTER_HOLE_ID, ALL_RAW_VALUE_LANES_MASK),
        _ => return None,
    };
    Some(RawValueHoleSpec { id, lane_mask })
}
