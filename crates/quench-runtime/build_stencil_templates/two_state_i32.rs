pub(super) const AARCH64: &str = r##"#![no_std]
use core::arch::global_asm;

const STATUS_OK: u32 = 1;
const STATUS_INTERRUPT: u32 = 4;
const INDEX_STEP: i32 = 1;

#[repr(C)]
struct TwoStateI32Context {
    index: i32,
    end: i32,
    first: i32,
    second: i32,
    sum_mask: i32,
    index_mask: i32,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_two_state_i32_loop
q_two_state_i32_loop:
    ldr w2, [x0, #{index}]
    ldr w3, [x0, #{end}]
    ldr w4, [x0, #{first}]
    ldr w5, [x0, #{second}]
    ldr w6, [x0, #{sum_mask}]
    ldr w7, [x0, #{index_mask}]
2:
    cmp w2, w3
    b.ge 4f
    add w8, w4, w5
    and w8, w8, w6
    mov w4, w5
    and w10, w2, w7
    eor w5, w8, w10
    add w2, w2, #{index_step}
    ldr x9, [x0, #{interrupt}]
    ldrb w11, [x9]
    cbnz w11, 3f
    b 2b
3:
    str w2, [x0, #{index}]
    str w4, [x0, #{first}]
    str w5, [x0, #{second}]
    mov w0, #{status_interrupt}
    ret
4:
    str w2, [x0, #{index}]
    str w4, [x0, #{first}]
    str w5, [x0, #{second}]
    mov w0, #{status_ok}
    ret
q_two_state_i32_loop_end:
"#,
    index = const core::mem::offset_of!(TwoStateI32Context, index),
    end = const core::mem::offset_of!(TwoStateI32Context, end),
    first = const core::mem::offset_of!(TwoStateI32Context, first),
    second = const core::mem::offset_of!(TwoStateI32Context, second),
    sum_mask = const core::mem::offset_of!(TwoStateI32Context, sum_mask),
    index_mask = const core::mem::offset_of!(TwoStateI32Context, index_mask),
    interrupt = const core::mem::offset_of!(TwoStateI32Context, interrupt),
    index_step = const INDEX_STEP,
    status_ok = const STATUS_OK,
    status_interrupt = const STATUS_INTERRUPT,
);
"##;
