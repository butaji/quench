pub(crate) fn aarch64_head() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_fallthrough_head\nq_fallthrough_head:\n  fadd d0, d0, d1\n  b q_fallthrough_tail\n  b q_fallthrough_tail\nq_fallthrough_head_end:\n\"#);\n"
}

pub(crate) fn aarch64_tail() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_fallthrough_tail\nq_fallthrough_tail:\n  ret\nq_fallthrough_tail_end:\n\"#);\n"
}

const AARCH64_ARRAY_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;
global_asm!(r#"
.text
.p2align 2
.globl q_array_numeric_loop
q_array_numeric_loop:
  ldr x1, [x0, #16]
  ldr x2, [x0, #24]
  ldr d0, [x0, #40]
  fmov d1, d0
  b 1f
1:
  cmp x1, x2
  b.hs 2f
  ldr x3, [x0]
  add x4, x3, x1, lsl #3
  ldr d1, [x4]
  ldr d2, [x0, #32]
  fadd d1, d1, d2
  str d1, [x4]
  add x1, x1, #1
  str x1, [x0, #16]
  ldr x5, [x0, #48]
  ldrb w6, [x5]
  cbnz w6, 3f
  b 1b
2:
  str d1, [x0, #40]
  mov w0, #1
  ret
3:
  str d1, [x0, #40]
  mov w0, #4
  ret
q_array_numeric_loop_end:
"#);
"##;

const AARCH64_PROTOTYPE_PROPERTY: &str = r##"#![no_std]
use core::arch::global_asm;
global_asm!(r#"
.text
.p2align 2
.globl q_prototype_property
q_prototype_property:
  ldr x1, [x0]
  ldr w2, [x1]
  ldr w3, [x0, #8]
  cmp w2, w3
  b.ne 9f
  ldr w4, [x0, #48]
  cmp w4, #1
  b.lo 9f
  cmp w4, #4
  b.hi 9f

  ldr x1, [x0, #56]
  ldr x2, [x1]
  ldr x3, [x0, #64]
  cmp x2, x3
  b.ne 9f
  ldr x1, [x0, #72]
  ldr w2, [x1]
  ldr w3, [x0, #80]
  cmp w2, w3
  b.ne 9f
  cmp w4, #1
  b.eq 8f

  ldr x1, [x0, #88]
  ldr x2, [x1]
  ldr x3, [x0, #96]
  cmp x2, x3
  b.ne 9f
  ldr x1, [x0, #104]
  ldr w2, [x1]
  ldr w3, [x0, #112]
  cmp w2, w3
  b.ne 9f
  cmp w4, #2
  b.eq 8f

  ldr x1, [x0, #120]
  ldr x2, [x1]
  ldr x3, [x0, #128]
  cmp x2, x3
  b.ne 9f
  ldr x1, [x0, #136]
  ldr w2, [x1]
  ldr w3, [x0, #144]
  cmp w2, w3
  b.ne 9f
  cmp w4, #3
  b.eq 8f

  ldr x1, [x0, #152]
  ldr x2, [x1]
  ldr x3, [x0, #160]
  cmp x2, x3
  b.ne 9f
  ldr x1, [x0, #168]
  ldr w2, [x1]
  ldr w3, [x0, #176]
  cmp w2, w3
  b.ne 9f
  cmp w4, #4
  b.eq 8f

8:
  ldr x1, [x0, #16]
  ldrb w2, [x1]
  cmp w2, #1
  b.ne 9f
  ldr x1, [x0, #24]
  ldrb w2, [x1]
  cmp w2, #1
  b.ne 9f
  ldr x1, [x0, #32]
  ldr x2, [x1]
  str x2, [x0, #40]
  mov w0, #1
  ret
9:
  mov w0, #0
  ret
q_prototype_property_end:
"#);
"##;

const AARCH64_PROPERTY_READ: &str = r##"#![no_std]
use core::arch::global_asm;
global_asm!(r#"
.text
.p2align 2
.globl q_property
q_property:
  ldr x1, [x0]
  ldr w2, [x1]
  ldr w3, [x0, #8]
  cmp w2, w3
  b.ne 1f
  ldr x1, [x0, #16]
  ldrb w2, [x1]
  cmp w2, #1
  b.ne 1f
  ldr x1, [x0, #24]
  ldrb w2, [x1]
  cmp w2, #1
  b.ne 1f
  ldr x1, [x0, #32]
  ldr x2, [x1]
  str x2, [x0, #40]
  mov w0, #1
  ret
1:
  mov w0, #0
  ret
q_property_end:
"#);
"##;

const AARCH64_PROPERTY_WRITE: &str = r##"#![no_std]
use core::arch::global_asm;
global_asm!(r#"
.text
.p2align 2
.globl q_store_property
q_store_property:
  ldr x1, [x0]
  ldr w2, [x1]
  ldr w3, [x0, #8]
  cmp w2, w3
  b.ne 1f
  ldr x1, [x0, #16]
  ldrb w2, [x1]
  cmp w2, #1
  b.ne 1f
  ldr x1, [x0, #24]
  ldrb w2, [x1]
  cmp w2, #1
  b.ne 1f
  ldr x1, [x0, #32]
  ldr x2, [x0, #40]
  str x2, [x1]
  mov w0, #1
  ret
1:
  mov w0, #0
  ret
q_store_property_end:
"#);
"##;

pub(crate) fn whole_region(name: &str) -> Option<&'static str> {
    match name {
        "array_numeric_loop" => Some(AARCH64_ARRAY_LOOP),
        "property" => Some(AARCH64_PROPERTY_READ),
        "prototype_property" => Some(AARCH64_PROTOTYPE_PROPERTY),
        "store_property" => Some(AARCH64_PROPERTY_WRITE),
        _ => None,
    }
}
