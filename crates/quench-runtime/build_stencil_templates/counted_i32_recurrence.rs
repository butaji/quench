pub(super) const AARCH64: &str = r##"#![no_std]
use core::arch::global_asm;

const STATUS_OK: u32 = 1;
const STATUS_INTERRUPT: u32 = 4;
const INDEX_STEP: i32 = 1;

#[repr(C)]
struct CountedI32Context {
    index: i32,
    end: i32,
    value: i32,
    shift: u32,
    multiplier: i32,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_counted_i32_recurrence
q_counted_i32_recurrence:
    ldr w2, [x0, #{index}]
    ldr w3, [x0, #{end}]
    ldr w4, [x0, #{value}]
    ldr w5, [x0, #{shift}]
    ldr w6, [x0, #{multiplier}]
1:
    cmp w2, w3
    b.ge 3f
    lsr w7, w4, w5
    eor w4, w4, w7
    mul w4, w4, w6
    add w2, w2, #{index_step}
    ldr x7, [x0, #{interrupt}]
    ldrb w7, [x7]
    cbnz w7, 2f
    b 1b
2:
    str w2, [x0, #{index}]
    str w4, [x0, #{value}]
    mov w0, #{status_interrupt}
    ret
3:
    str w2, [x0, #{index}]
    str w4, [x0, #{value}]
    mov w0, #{status_ok}
    ret
q_counted_i32_recurrence_end:
"#,
    index = const core::mem::offset_of!(CountedI32Context, index),
    end = const core::mem::offset_of!(CountedI32Context, end),
    value = const core::mem::offset_of!(CountedI32Context, value),
    shift = const core::mem::offset_of!(CountedI32Context, shift),
    multiplier = const core::mem::offset_of!(CountedI32Context, multiplier),
    interrupt = const core::mem::offset_of!(CountedI32Context, interrupt),
    index_step = const INDEX_STEP,
    status_ok = const STATUS_OK,
    status_interrupt = const STATUS_INTERRUPT,
);
"##;
