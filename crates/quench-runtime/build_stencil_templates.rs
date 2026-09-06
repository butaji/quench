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

fn tagged_word_source(name: &str) -> String {
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  ldr x0, [x0]\n  ret\nq_{name}_end:\n\"#);\n"
    )
}

fn truthy_pointer_source(name: &str) -> String {
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  mov w0, #1\n  ret\nq_{name}_end:\n\"#);\n"
    )
}

fn array_get_source(name: &str) -> String {
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  ldr x1, [x0]\n  ldr d0, [x1]\n  str d0, [x0, #8]\n  mov w0, #1\n  ret\nq_{name}_end:\n\"#);\n"
    )
}

fn array_set_source(name: &str) -> String {
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  ldr x1, [x0]\n  ldr d0, [x0, #8]\n  str d0, [x1]\n  mov w0, #1\n  ret\nq_{name}_end:\n\"#);\n"
    )
}

fn array_get_inc_source(name: &str) -> String {
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  ldr x1, [x0]\n  ldr d0, [x1]\n  str d0, [x0, #8]\n  ldr x1, [x0, #16]\n  add x1, x1, #1\n  str x1, [x0, #24]\n  mov w0, #1\n  ret\nq_{name}_end:\n\"#);\n"
    )
}

fn array_update_source(name: &str) -> String {
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  ldr x1, [x0]\n  ldr x2, [x0, #8]\n  ldr x3, [x0, #16]\n  add x4, x1, x3, lsl #3\n  ldr d0, [x4]\n  ldr d1, [x0, #24]\n  fadd d0, d0, d1\n  str d0, [x4]\n  str d0, [x0, #32]\n  mov w0, #1\n  ret\nq_{name}_end:\n\"#);\n"
    )
}

fn load_const_source(name: &str) -> String {
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  ldr x0, 1f\n  ret\n.p2align 3\nq_{name}_hole_0:\n1:\n  .quad 0\nq_{name}_end:\n\"#);\n"
    )
}

fn truthy_word_source(name: &str) -> String {
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  ldr x1, 1f\n  cmp x0, x1\n  cset w0, eq\n  ret\n.p2align 3\nq_{name}_hole_0:\n1:\n  .quad 0\nq_{name}_end:\n\"#);\n"
    )
}

fn nullish_word_source(name: &str) -> String {
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  ldr x1, 1f\n  orr x0, x0, #1\n  cmp x0, x1\n  cset w0, eq\n  ret\n.p2align 3\nq_{name}_hole_0:\n1:\n  .quad 0\nq_{name}_end:\n\"#);\n"
    )
}

pub(crate) fn assembly_source(recipe: crate::build_stencil_contract::RustAssemblyRecipe) -> String {
    use crate::build_stencil_contract::RustAssemblyRecipe::*;
    match recipe {
        ArrayNumericLoop => AARCH64_ARRAY_LOOP.to_owned(),
        Property => AARCH64_PROPERTY_READ.to_owned(),
        PrototypeProperty => AARCH64_PROTOTYPE_PROPERTY.to_owned(),
        StoreProperty => AARCH64_PROPERTY_WRITE.to_owned(),
        ArrayGetNumber => array_get_source(recipe.name()),
        ArraySetNumber => array_set_source(recipe.name()),
        ArrayGetIncNumber => array_get_inc_source(recipe.name()),
        ArrayNumericUpdate | ArrayNumericUpdateConst | ArrayLoopBody => {
            array_update_source(recipe.name())
        }
        Move | LoadLocal | StoreLocal => tagged_word_source(recipe.name()),
        TruthyPointer => truthy_pointer_source(recipe.name()),
        LoadConst => load_const_source(recipe.name()),
        NullishWord => nullish_word_source(recipe.name()),
        TruthyWord => truthy_word_source(recipe.name()),
    }
}
