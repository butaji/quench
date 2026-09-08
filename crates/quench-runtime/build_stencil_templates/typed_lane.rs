pub(super) const AARCH64: &str = r##"#![no_std]
use core::arch::global_asm;

const STATUS_OK: u32 = 1;
const STATUS_INTERRUPT: u32 = 4;
const INDEX_STEP: u32 = 1;
const LANE_ADDRESS_SHIFT: u32 = 2;

#[repr(C)]
struct TypedLaneContext {
    values: *mut i32,
    index: u32,
    end: u32,
    xor_mask: i32,
    adjustment: i32,
    total: f64,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_typed_lane_loop
q_typed_lane_loop:
  ldr x1, [x0, #{values}]
  ldr w2, [x0, #{index}]
  ldr w3, [x0, #{end}]
  ldr w4, [x0, #{xor_mask}]
  ldr w5, [x0, #{adjustment}]
  ldr d0, [x0, #{total}]
1:
  cmp w2, w3
  b.ge 9f
  eor w6, w2, w4
  add w6, w6, w5
  str w6, [x1, x2, lsl #{lane_address_shift}]
  scvtf d1, w6
  fmul d1, d1, d1
  fadd d0, d0, d1
  add w2, w2, #{index_step}
  str w2, [x0, #{index}]
  str d0, [x0, #{total}]
  ldr x7, [x0, #{interrupt}]
  ldrb w8, [x7]
  cbnz w8, 10f
  b 1b
9:
  str d0, [x0, #{total}]
  mov w0, #{status_ok}
  ret
10:
  mov w0, #{status_interrupt}
  ret
q_typed_lane_loop_end:
"#,
    values = const core::mem::offset_of!(TypedLaneContext, values),
    index = const core::mem::offset_of!(TypedLaneContext, index),
    end = const core::mem::offset_of!(TypedLaneContext, end),
    xor_mask = const core::mem::offset_of!(TypedLaneContext, xor_mask),
    adjustment = const core::mem::offset_of!(TypedLaneContext, adjustment),
    total = const core::mem::offset_of!(TypedLaneContext, total),
    interrupt = const core::mem::offset_of!(TypedLaneContext, interrupt),
    lane_address_shift = const LANE_ADDRESS_SHIFT,
    index_step = const INDEX_STEP,
    status_ok = const STATUS_OK,
    status_interrupt = const STATUS_INTERRUPT,
);
"##;
