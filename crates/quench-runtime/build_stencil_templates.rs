pub(crate) fn aarch64_head() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_fallthrough_head\nq_fallthrough_head:\n  fadd d0, d0, d1\n  b q_fallthrough_tail\n  b q_fallthrough_tail\nq_fallthrough_head_end:\n\"#);\n"
}

pub(crate) fn aarch64_tail() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_fallthrough_tail\nq_fallthrough_tail:\n  ret\nq_fallthrough_tail_end:\n\"#);\n"
}

fn aarch64_sub_head() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_sub_fallthrough_head\nq_sub_fallthrough_head:\n  fsub d0, d0, d1\n  b q_fallthrough_tail\nq_sub_fallthrough_head_end:\n\"#);\n"
}

fn aarch64_mul_head() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_mul_fallthrough_head\nq_mul_fallthrough_head:\n  fmul d0, d0, d1\n  b q_fallthrough_tail\nq_mul_fallthrough_head_end:\n\"#);\n"
}

fn aarch64_div_head() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_div_fallthrough_head\nq_div_fallthrough_head:\n  fdiv d0, d0, d1\n  b q_fallthrough_tail\nq_div_fallthrough_head_end:\n\"#);\n"
}

fn aarch64_add_chain_head() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_add_chain_head\nq_add_chain_head:\n  fadd d0, d0, d1\n  b q_add_chain_tail\nq_add_chain_head_end:\n\"#);\n"
}

fn aarch64_add_chain_tail() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_add_chain_tail\nq_add_chain_tail:\n  fadd d0, d0, d2\n  ret\nq_add_chain_tail_end:\n\"#);\n"
}

pub(crate) fn fragment_sources(
    recipe: super::RustAssemblyRecipe,
) -> Option<(&'static str, &'static str)> {
    use super::RustAssemblyRecipe::{
        AddChain, DivFallthrough, Fallthrough, MulFallthrough, SubFallthrough,
    };
    match recipe {
        Fallthrough => Some((aarch64_head(), aarch64_tail())),
        SubFallthrough => Some((aarch64_sub_head(), aarch64_tail())),
        MulFallthrough => Some((aarch64_mul_head(), aarch64_tail())),
        DivFallthrough => Some((aarch64_div_head(), aarch64_tail())),
        AddChain => Some((aarch64_add_chain_head(), aarch64_add_chain_tail())),
        _ => None,
    }
}

pub(crate) fn control_fragment_source(recipe: super::RustAssemblyRecipe) -> Option<&'static str> {
    use super::RustAssemblyRecipe::{BoolBranch, TruthyBoolBranch, WordConstFragment};
    match recipe {
        BoolBranch => Some(
            "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_bool_branch\nq_bool_branch:\n  cbnz w0, 1f\n  b q_bool_branch_false\n1:\n  b q_bool_branch_true\nq_bool_branch_end:\n\"#);\n",
        ),
        TruthyBoolBranch => Some(
            "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_truthy_bool_branch\nq_truthy_bool_branch:\n  ldr x1, 2f\n  cmp x0, x1\n  b.eq 1f\n  b q_truthy_bool_branch_false\n1:\n  b q_truthy_bool_branch_true\n.p2align 3\nq_truthy_bool_branch_hole_0:\n2:\n  .quad 0\nq_truthy_bool_branch_end:\n\"#);\n",
        ),
        WordConstFragment => Some(
            "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_word_const_fragment\nq_word_const_fragment:\n  ldr x0, 1f\n  b q_word_const_fragment_next\n.p2align 3\nq_word_const_fragment_hole_0:\n1:\n  .quad 0\nq_word_const_fragment_end:\n\"#);\n",
        ),
        _ => None,
    }
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

const AARCH64_ARRAY_FILL_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct ArrayLoopContext {
    data: *mut f64,
    len: usize,
    index: usize,
    end: usize,
    addend: f64,
    result: f64,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_array_numeric_fill_loop
q_array_numeric_fill_loop:
  ldr x1, [x0, #{index}]
  ldr x2, [x0, #{end}]
  ldr d1, [x0, #{value}]
1:
  cmp x1, x2
  b.hs 2f
  ldr x3, [x0, #{data}]
  add x4, x3, x1, lsl #3
  str d1, [x4]
  add x1, x1, #1
  str x1, [x0, #{index}]
  ldr x5, [x0, #{interrupt}]
  ldrb w6, [x5]
  cbnz w6, 3f
  b 1b
2:
  str d1, [x0, #{result}]
  mov w0, #1
  ret
3:
  str d1, [x0, #{result}]
  mov w0, #4
  ret
q_array_numeric_fill_loop_end:
"#,
    data = const core::mem::offset_of!(ArrayLoopContext, data),
    index = const core::mem::offset_of!(ArrayLoopContext, index),
    end = const core::mem::offset_of!(ArrayLoopContext, end),
    value = const core::mem::offset_of!(ArrayLoopContext, addend),
    result = const core::mem::offset_of!(ArrayLoopContext, result),
    interrupt = const core::mem::offset_of!(ArrayLoopContext, interrupt),
);
"##;

const AARCH64_AFFINE_I32_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;
global_asm!(r#"
.text
.p2align 2
.globl q_affine_i32_loop
q_affine_i32_loop:
  ldr x1, [x0]
  ldr x2, [x0, #8]
  ldr w3, [x0, #16]
  ldr w4, [x0, #20]
  ldr w5, [x0, #24]
1:
  cmp x1, x2
  b.hs 2f
  mul w3, w3, w4
  add w3, w3, w5
  add x1, x1, #1
  str x1, [x0]
  ldr x6, [x0, #32]
  ldrb w7, [x6]
  cbnz w7, 3f
  b 1b
2:
  str w3, [x0, #16]
  mov w0, #1
  ret
3:
  str w3, [x0, #16]
  mov w0, #4
  ret
q_affine_i32_loop_end:
"#);
"##;

const AARCH64_I32_COUNTER_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct CounterLoopContext {
    index: usize,
    end: usize,
    value: i32,
    multiplier: i32,
    counter: i32,
    decrement: i32,
    addend: i32,
    _padding: u32,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_i32_counter_loop
q_i32_counter_loop:
  ldr x1, [x0, #{index}]
  ldr x2, [x0, #{end}]
  ldr w3, [x0, #{value}]
  ldr w4, [x0, #{multiplier}]
  ldr w5, [x0, #{counter}]
  ldr w6, [x0, #{decrement}]
  ldr w7, [x0, #{addend}]
1:
  cmp x1, x2
  b.hs 2f
  sub w5, w5, w6
  mul w3, w3, w4
  add w3, w3, w5
  add w3, w3, w7
  add x1, x1, #1
  str x1, [x0, #{index}]
  str w3, [x0, #{value}]
  str w5, [x0, #{counter}]
  ldr x8, [x0, #{interrupt}]
  ldrb w9, [x8]
  cbnz w9, 3f
  b 1b
2:
  str x1, [x0, #{index}]
  str w3, [x0, #{value}]
  str w5, [x0, #{counter}]
  mov w0, #1
  ret
3:
  mov w0, #4
  ret
q_i32_counter_loop_end:
"#,
    index = const core::mem::offset_of!(CounterLoopContext, index),
    end = const core::mem::offset_of!(CounterLoopContext, end),
    value = const core::mem::offset_of!(CounterLoopContext, value),
    multiplier = const core::mem::offset_of!(CounterLoopContext, multiplier),
    counter = const core::mem::offset_of!(CounterLoopContext, counter),
    decrement = const core::mem::offset_of!(CounterLoopContext, decrement),
    addend = const core::mem::offset_of!(CounterLoopContext, addend),
    interrupt = const core::mem::offset_of!(CounterLoopContext, interrupt),
);
"##;

const AARCH64_BOOLEAN_REDUCTION_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct BooleanReductionContext {
    index: i32,
    end: i32,
    count: i32,
    left_kind: u32,
    left_operand: i32,
    left_expected: i32,
    right_kind: u32,
    right_operand: i32,
    right_expected: i32,
    truth_table: u32,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_boolean_reduction_loop
q_boolean_reduction_loop:
  ldr w1, [x0, #{index}]
  ldr w2, [x0, #{end}]
  ldr w3, [x0, #{count}]
1:
  cmp w1, w2
  b.ge 5f
  ldr w4, [x0, #{left_kind}]
  ldr w5, [x0, #{left_operand}]
  ldr w6, [x0, #{left_expected}]
  cmp w4, #1
  b.ne 2f
  and w7, w1, w5
  b 3f
2:
  sdiv w8, w1, w5
  msub w7, w8, w5, w1
3:
  cmp w7, w6
  cset w7, eq
  ldr w4, [x0, #{right_kind}]
  ldr w5, [x0, #{right_operand}]
  ldr w6, [x0, #{right_expected}]
  cmp w4, #1
  b.ne 4f
  and w8, w1, w5
  b 6f
4:
  sdiv w9, w1, w5
  msub w8, w9, w5, w1
6:
  cmp w8, w6
  cset w8, eq
  add w7, w7, w7
  orr w7, w7, w8
  ldr w9, [x0, #{truth_table}]
  lsr w9, w9, w7
  and w9, w9, #1
  add w3, w3, w9
  add w1, w1, #1
  str w1, [x0, #{index}]
  str w3, [x0, #{count}]
  ldr x10, [x0, #{interrupt}]
  ldrb w11, [x10]
  cbnz w11, 7f
  b 1b
5:
  mov w0, #1
  ret
7:
  mov w0, #4
  ret
q_boolean_reduction_loop_end:
"#,
    index = const core::mem::offset_of!(BooleanReductionContext, index),
    end = const core::mem::offset_of!(BooleanReductionContext, end),
    count = const core::mem::offset_of!(BooleanReductionContext, count),
    left_kind = const core::mem::offset_of!(BooleanReductionContext, left_kind),
    left_operand = const core::mem::offset_of!(BooleanReductionContext, left_operand),
    left_expected = const core::mem::offset_of!(BooleanReductionContext, left_expected),
    right_kind = const core::mem::offset_of!(BooleanReductionContext, right_kind),
    right_operand = const core::mem::offset_of!(BooleanReductionContext, right_operand),
    right_expected = const core::mem::offset_of!(BooleanReductionContext, right_expected),
    truth_table = const core::mem::offset_of!(BooleanReductionContext, truth_table),
    interrupt = const core::mem::offset_of!(BooleanReductionContext, interrupt),
);
"##;

const AARCH64_BRANCH_RECURRENCE_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct BranchRecurrenceContext {
    index: i32,
    end: i32,
    state: f64,
    score: i64,
    multiplier: f64,
    addend: f64,
    predicate_mask: u32,
    predicate_expected: u32,
    predicate_invert: u32,
    true_mask: u32,
    true_sign: i32,
    false_mask: u32,
    false_sign: i32,
    _padding: u32,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_branch_recurrence_loop
q_branch_recurrence_loop:
  ldr w1, [x0, #{index}]
  ldr w2, [x0, #{end}]
  ldr d0, [x0, #{state}]
  ldr x3, [x0, #{score}]
  ldr d1, [x0, #{multiplier}]
  ldr d2, [x0, #{addend}]
1:
  cmp w1, w2
  b.ge 5f
  fmul d0, d0, d1
  fadd d0, d0, d2
  fcvtzu x4, d0
  ucvtf d0, w4
  ldr w5, [x0, #{predicate_mask}]
  and w6, w4, w5
  ldr w7, [x0, #{predicate_expected}]
  cmp w6, w7
  cset w6, eq
  ldr w7, [x0, #{predicate_invert}]
  eor w6, w6, w7
  cbz w6, 2f
  ldr w7, [x0, #{true_mask}]
  ldr w8, [x0, #{true_sign}]
  b 3f
2:
  ldr w7, [x0, #{false_mask}]
  ldr w8, [x0, #{false_sign}]
3:
  and w7, w1, w7
  sxtw x7, w7
  sxtw x8, w8
  madd x3, x7, x8, x3
  add w1, w1, #1
  str w1, [x0, #{index}]
  str d0, [x0, #{state}]
  str x3, [x0, #{score}]
  ldr x9, [x0, #{interrupt}]
  ldrb w10, [x9]
  cbnz w10, 4f
  b 1b
4:
  mov w0, #4
  ret
5:
  mov w0, #1
  ret
q_branch_recurrence_loop_end:
"#,
    index = const core::mem::offset_of!(BranchRecurrenceContext, index),
    end = const core::mem::offset_of!(BranchRecurrenceContext, end),
    state = const core::mem::offset_of!(BranchRecurrenceContext, state),
    score = const core::mem::offset_of!(BranchRecurrenceContext, score),
    multiplier = const core::mem::offset_of!(BranchRecurrenceContext, multiplier),
    addend = const core::mem::offset_of!(BranchRecurrenceContext, addend),
    predicate_mask = const core::mem::offset_of!(BranchRecurrenceContext, predicate_mask),
    predicate_expected = const core::mem::offset_of!(BranchRecurrenceContext, predicate_expected),
    predicate_invert = const core::mem::offset_of!(BranchRecurrenceContext, predicate_invert),
    true_mask = const core::mem::offset_of!(BranchRecurrenceContext, true_mask),
    true_sign = const core::mem::offset_of!(BranchRecurrenceContext, true_sign),
    false_mask = const core::mem::offset_of!(BranchRecurrenceContext, false_mask),
    false_sign = const core::mem::offset_of!(BranchRecurrenceContext, false_sign),
    interrupt = const core::mem::offset_of!(BranchRecurrenceContext, interrupt),
);
"##;

const AARCH64_NESTED_XOR_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct NestedXorContext {
    indices: [i32; 3],
    starts: [i32; 3],
    ends: [i32; 3],
    mask: u32,
    total: i64,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_nested_xor_loop
q_nested_xor_loop:
  ldr w1, [x0, #{index0}]
  ldr w2, [x0, #{index1}]
  ldr w3, [x0, #{index2}]
  ldr x4, [x0, #{total}]
1:
  ldr w5, [x0, #{end0}]
  cmp w1, w5
  b.ge 6f
  ldr w5, [x0, #{end1}]
  cmp w2, w5
  b.ge 5f
  ldr w5, [x0, #{end2}]
  cmp w3, w5
  b.ge 4f
  eor w5, w1, w2
  eor w5, w5, w3
  ldr w6, [x0, #{mask}]
  and w5, w5, w6
  add x4, x4, x5
  add w3, w3, #1
  b 8f
4:
  add w2, w2, #1
  ldr w3, [x0, #{start2}]
  b 8f
5:
  add w1, w1, #1
  ldr w2, [x0, #{start1}]
  ldr w3, [x0, #{start2}]
  b 8f
6:
  str w1, [x0, #{index0}]
  str w2, [x0, #{index1}]
  str w3, [x0, #{index2}]
  str x4, [x0, #{total}]
  mov w0, #1
  ret
7:
  mov w0, #4
  ret
8:
  str w1, [x0, #{index0}]
  str w2, [x0, #{index1}]
  str w3, [x0, #{index2}]
  str x4, [x0, #{total}]
  ldr x7, [x0, #{interrupt}]
  ldrb w8, [x7]
  cbnz w8, 7b
  b 1b
q_nested_xor_loop_end:
"#,
    index0 = const core::mem::offset_of!(NestedXorContext, indices),
    index1 = const core::mem::offset_of!(NestedXorContext, indices) + core::mem::size_of::<i32>(),
    index2 = const core::mem::offset_of!(NestedXorContext, indices) + 2 * core::mem::size_of::<i32>(),
    start1 = const core::mem::offset_of!(NestedXorContext, starts) + core::mem::size_of::<i32>(),
    start2 = const core::mem::offset_of!(NestedXorContext, starts) + 2 * core::mem::size_of::<i32>(),
    end0 = const core::mem::offset_of!(NestedXorContext, ends),
    end1 = const core::mem::offset_of!(NestedXorContext, ends) + core::mem::size_of::<i32>(),
    end2 = const core::mem::offset_of!(NestedXorContext, ends) + 2 * core::mem::size_of::<i32>(),
    mask = const core::mem::offset_of!(NestedXorContext, mask),
    total = const core::mem::offset_of!(NestedXorContext, total),
    interrupt = const core::mem::offset_of!(NestedXorContext, interrupt),
);
"##;

const AARCH64_SWITCH_REDUCTION_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

const ACTION_ADD_SIGNED: u32 = 1;
const ACTION_XOR: u32 = 2;
const ACTION_ADD_INDEX_MASKED: u32 = 3;
const ACTION_SHIFT_LEFT_OR: u32 = 4;
const ACTION_SHIFT_RIGHT_UNSIGNED: u32 = 5;

#[repr(C)]
struct SwitchReductionContext {
    index: i32, end: i32, total: i64,
    selector_sign: i32, selector_bias: i32, divisor: i32, case_count: u32,
    case_values: [i32; 8], action_kinds: [u32; 8],
    action_a: [i32; 8], action_b: [i32; 8],
    default_kind: u32, default_a: i32, default_b: i32, _padding: u32,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_switch_reduction_loop
q_switch_reduction_loop:
  ldr w1, [x0, #{index}]
  ldr w2, [x0, #{end}]
  ldr x4, [x0, #{total}]
1:
  cmp w1, w2
  b.ge 9f
  ldr w5, [x0, #{selector_sign}]
  ldr w6, [x0, #{selector_bias}]
  madd w5, w1, w5, w6
  ldr w6, [x0, #{divisor}]
  sdiv w7, w5, w6
  msub w5, w7, w6, w5
  ldr w8, [x0, #{case_count}]
  mov w9, #0
2:
  cmp w9, w8
  b.ge 4f
  add x10, x0, #{case_values}
  add x10, x10, x9, lsl #2
  ldr w11, [x10]
  cmp w5, w11
  b.eq 3f
  add w9, w9, #1
  b 2b
3:
  add x10, x0, #{action_kinds}
  add x10, x10, x9, lsl #2
  ldr w11, [x10]
  add x10, x0, #{action_a}
  add x10, x10, x9, lsl #2
  ldr w12, [x10]
  add x10, x0, #{action_b}
  add x10, x10, x9, lsl #2
  ldr w13, [x10]
  b 5f
4:
  ldr w11, [x0, #{default_kind}]
  ldr w12, [x0, #{default_a}]
  ldr w13, [x0, #{default_b}]
5:
  cmp w11, #{action_add_signed}
  b.ne 6f
  sxtw x12, w12
  add x4, x4, x12
  b 8f
6:
  cmp w11, #{action_xor}
  b.ne 10f
  eor w4, w4, w12
  sxtw x4, w4
  b 8f
10:
  cmp w11, #{action_add_index_masked}
  b.ne 11f
  and w12, w1, w12
  sxtw x12, w12
  cmp w13, #0
  b.ge 15f
  sub x4, x4, x12
  b 8f
15:
  add x4, x4, x12
  b 8f
11:
  cmp w11, #{action_shift_left_or}
  b.ne 12f
  lslv w4, w4, w12
  orr w4, w4, w13
  sxtw x4, w4
  b 8f
12:
  cmp w11, #{action_shift_right_unsigned}
  b.ne 13f
  lsrv w4, w4, w12
  b 8f
13:
  mov w0, #2
  ret
8:
  add w1, w1, #1
  str w1, [x0, #{index}]
  str x4, [x0, #{total}]
  ldr x14, [x0, #{interrupt}]
  ldrb w15, [x14]
  cbnz w15, 14f
  b 1b
9:
  sxtw x4, w4
  str x4, [x0, #{total}]
  mov w0, #1
  ret
14:
  mov w0, #4
  ret
q_switch_reduction_loop_end:
"#,
    index = const core::mem::offset_of!(SwitchReductionContext, index),
    end = const core::mem::offset_of!(SwitchReductionContext, end),
    total = const core::mem::offset_of!(SwitchReductionContext, total),
    selector_sign = const core::mem::offset_of!(SwitchReductionContext, selector_sign),
    selector_bias = const core::mem::offset_of!(SwitchReductionContext, selector_bias),
    divisor = const core::mem::offset_of!(SwitchReductionContext, divisor),
    case_count = const core::mem::offset_of!(SwitchReductionContext, case_count),
    case_values = const core::mem::offset_of!(SwitchReductionContext, case_values),
    action_kinds = const core::mem::offset_of!(SwitchReductionContext, action_kinds),
    action_a = const core::mem::offset_of!(SwitchReductionContext, action_a),
    action_b = const core::mem::offset_of!(SwitchReductionContext, action_b),
    default_kind = const core::mem::offset_of!(SwitchReductionContext, default_kind),
    default_a = const core::mem::offset_of!(SwitchReductionContext, default_a),
    default_b = const core::mem::offset_of!(SwitchReductionContext, default_b),
    interrupt = const core::mem::offset_of!(SwitchReductionContext, interrupt),
    action_add_signed = const ACTION_ADD_SIGNED,
    action_xor = const ACTION_XOR,
    action_add_index_masked = const ACTION_ADD_INDEX_MASKED,
    action_shift_left_or = const ACTION_SHIFT_LEFT_OR,
    action_shift_right_unsigned = const ACTION_SHIFT_RIGHT_UNSIGNED,
);
"##;

const AARCH64_NUMERIC_INTEGER_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct IntegerLoopContext {
    index: usize,
    end: usize,
    value: i32,
    multiplier: i32,
    _unused: i32,
    _padding: u32,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_numeric_integer_loop
q_numeric_integer_loop:
  ldr x1, [x0, #{index}]
  ldr x2, [x0, #{end}]
  ldr w3, [x0, #{value}]
  ldr w4, [x0, #{multiplier}]
1:
  cmp x1, x2
  b.hs 2f
  mul w3, w3, w4
  add w3, w3, w1
  add x1, x1, #1
  str x1, [x0, #{index}]
  ldr x5, [x0, #{interrupt}]
  ldrb w6, [x5]
  cbnz w6, 3f
  b 1b
2:
  str w3, [x0, #{value}]
  mov w0, #1
  ret
3:
  str w3, [x0, #{value}]
  mov w0, #4
  ret
q_numeric_integer_loop_end:
"#,
    index = const core::mem::offset_of!(IntegerLoopContext, index),
    end = const core::mem::offset_of!(IntegerLoopContext, end),
    value = const core::mem::offset_of!(IntegerLoopContext, value),
    multiplier = const core::mem::offset_of!(IntegerLoopContext, multiplier),
    interrupt = const core::mem::offset_of!(IntegerLoopContext, interrupt),
);
"##;

const AARCH64_NUMERIC_FLOATING_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct FloatingLoopContext {
    index: usize,
    end: usize,
    value: f64,
    multiplier: f64,
    divisor: f64,
    modulus: usize,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_numeric_floating_loop
q_numeric_floating_loop:
  ldr x1, [x0, #{index}]
  ldr x2, [x0, #{end}]
  ldr d0, [x0, #{value}]
  ldr d1, [x0, #{multiplier}]
  ldr d3, [x0, #{divisor}]
  ldr x3, [x0, #{modulus}]
1:
  cmp x1, x2
  b.hs 2f
  fmul d0, d0, d1
  udiv x4, x1, x3
  msub x4, x4, x3, x1
  ucvtf d4, x4
  fdiv d4, d4, d3
  fadd d0, d0, d4
  add x1, x1, #1
  str x1, [x0, #{index}]
  ldr x5, [x0, #{interrupt}]
  ldrb w6, [x5]
  cbnz w6, 3f
  b 1b
2:
  str d0, [x0, #{value}]
  mov w0, #1
  ret
3:
  str d0, [x0, #{value}]
  mov w0, #4
  ret
q_numeric_floating_loop_end:
"#,
    index = const core::mem::offset_of!(FloatingLoopContext, index),
    end = const core::mem::offset_of!(FloatingLoopContext, end),
    value = const core::mem::offset_of!(FloatingLoopContext, value),
    multiplier = const core::mem::offset_of!(FloatingLoopContext, multiplier),
    divisor = const core::mem::offset_of!(FloatingLoopContext, divisor),
    modulus = const core::mem::offset_of!(FloatingLoopContext, modulus),
    interrupt = const core::mem::offset_of!(FloatingLoopContext, interrupt),
);
"##;

const AARCH64_NUMERIC_BITWISE_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct BitwiseLoopContext {
    index: usize,
    end: usize,
    value: i32,
    left_shift: u32,
    right_shift: u32,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_numeric_bitwise_loop
q_numeric_bitwise_loop:
  ldr x1, [x0, #{index}]
  ldr x2, [x0, #{end}]
  ldr w3, [x0, #{value}]
  ldr w5, [x0, #{left_shift}]
  ldr w6, [x0, #{right_shift}]
1:
  cmp x1, x2
  b.hs 2f
  lsl w4, w3, w5
  lsr w7, w3, w6
  eor w4, w4, w7
  eor w3, w4, w1
  add x1, x1, #1
  str x1, [x0, #{index}]
  ldr x4, [x0, #{interrupt}]
  ldrb w4, [x4]
  cbnz w4, 3f
  b 1b
2:
  str w3, [x0, #{value}]
  mov w0, #1
  ret
3:
  str w3, [x0, #{value}]
  mov w0, #4
  ret
q_numeric_bitwise_loop_end:
"#,
    index = const core::mem::offset_of!(BitwiseLoopContext, index),
    end = const core::mem::offset_of!(BitwiseLoopContext, end),
    value = const core::mem::offset_of!(BitwiseLoopContext, value),
    left_shift = const core::mem::offset_of!(BitwiseLoopContext, left_shift),
    right_shift = const core::mem::offset_of!(BitwiseLoopContext, right_shift),
    interrupt = const core::mem::offset_of!(BitwiseLoopContext, interrupt),
);
"##;

const AARCH64_NUMERIC_INDEPENDENT_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct IndependentLoopContext {
    index: usize,
    end: usize,
    left: i32,
    right: i32,
    multiplier: i32,
    _padding: u32,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_numeric_independent_loop
q_numeric_independent_loop:
  ldr x1, [x0, #{index}]
  ldr x2, [x0, #{end}]
  ldr w3, [x0, #{left}]
  ldr w4, [x0, #{right}]
  ldr w5, [x0, #{multiplier}]
1:
  cmp x1, x2
  b.hs 2f
  mul w3, w3, w5
  add w3, w3, w1
  mul w4, w4, w5
  add w4, w4, w1
  add x1, x1, #1
  str x1, [x0, #{index}]
  ldr x6, [x0, #{interrupt}]
  ldrb w7, [x6]
  cbnz w7, 3f
  b 1b
2:
  str w3, [x0, #{left}]
  str w4, [x0, #{right}]
  mov w0, #1
  ret
3:
  str w3, [x0, #{left}]
  str w4, [x0, #{right}]
  mov w0, #4
  ret
q_numeric_independent_loop_end:
"#,
    index = const core::mem::offset_of!(IndependentLoopContext, index),
    end = const core::mem::offset_of!(IndependentLoopContext, end),
    left = const core::mem::offset_of!(IndependentLoopContext, left),
    right = const core::mem::offset_of!(IndependentLoopContext, right),
    multiplier = const core::mem::offset_of!(IndependentLoopContext, multiplier),
    interrupt = const core::mem::offset_of!(IndependentLoopContext, interrupt),
);
"##;

const AARCH64_NUMERIC_MIXED_LOOP: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct MixedLoopContext {
    index: usize,
    end: usize,
    value: f64,
    exceptional_increment: f64,
    ordinary_increment: f64,
    period: usize,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_numeric_mixed_loop
q_numeric_mixed_loop:
  ldr x1, [x0, #{index}]
  ldr x2, [x0, #{end}]
  ldr d0, [x0, #{value}]
  ldr d1, [x0, #{exceptional_increment}]
  ldr d2, [x0, #{ordinary_increment}]
  ldr x3, [x0, #{period}]
1:
  cmp x1, x2
  b.hs 2f
  udiv x4, x1, x3
  msub x4, x4, x3, x1
  cmp x4, #0
  fcsel d3, d1, d2, eq
  fadd d0, d0, d3
  add x1, x1, #1
  str x1, [x0, #{index}]
  ldr x5, [x0, #{interrupt}]
  ldrb w6, [x5]
  cbnz w6, 3f
  b 1b
2:
  str d0, [x0, #{value}]
  mov w0, #1
  ret
3:
  str d0, [x0, #{value}]
  mov w0, #4
  ret
q_numeric_mixed_loop_end:
"#,
    index = const core::mem::offset_of!(MixedLoopContext, index),
    end = const core::mem::offset_of!(MixedLoopContext, end),
    value = const core::mem::offset_of!(MixedLoopContext, value),
    exceptional_increment = const core::mem::offset_of!(MixedLoopContext, exceptional_increment),
    ordinary_increment = const core::mem::offset_of!(MixedLoopContext, ordinary_increment),
    period = const core::mem::offset_of!(MixedLoopContext, period),
    interrupt = const core::mem::offset_of!(MixedLoopContext, interrupt),
);
"##;

fn compare_branch_source(name: &str, condition: &str, unordered_true: bool) -> String {
    let unordered = if unordered_true { "1f" } else { "2f" };
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  ldr d0, [x0]\n  ldr d1, [x0, #8]\n  fcmp d0, d1\n  b.vs {unordered}\n  b.{condition} 1f\n2:\n  mov w1, #0\n  ldr x2, [x0, #24]\n  b 3f\n1:\n  mov w1, #1\n  ldr x2, [x0, #16]\n3:\n  str x1, [x0, #32]\n  str x2, [x0, #40]\n  mov w0, #1\n  ret\nq_{name}_end:\n\"#);\n"
    )
}

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

fn return_word_source(name: &str) -> String {
    format!(
        "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_{name}\nq_{name}:\n  ret\nq_{name}_end:\n\"#);\n"
    )
}

pub(crate) fn assembly_source(recipe: super::RustAssemblyRecipe) -> String {
    use super::RustAssemblyRecipe::*;
    match recipe {
        Fallthrough | SubFallthrough | MulFallthrough | DivFallthrough | AddChain => {
            fragment_sources(recipe)
                .map(|(head, tail)| head.to_owned() + tail)
                .expect("declared fragment pair")
        }
        BoolBranch | TruthyBoolBranch | WordConstFragment => control_fragment_source(recipe)
            .expect("declared control fragment")
            .to_owned(),
        ReturnWord => return_word_source(recipe.name()),
        CompareEqualBranch => compare_branch_source(recipe.name(), "eq", false),
        CompareNotEqualBranch => compare_branch_source(recipe.name(), "ne", true),
        CompareLessBranch => compare_branch_source(recipe.name(), "lt", false),
        CompareLessEqualBranch => compare_branch_source(recipe.name(), "le", false),
        CompareGreaterBranch => compare_branch_source(recipe.name(), "gt", false),
        CompareGreaterEqualBranch => compare_branch_source(recipe.name(), "ge", false),
        ArrayNumericLoop => AARCH64_ARRAY_LOOP.to_owned(),
        ArrayNumericFillLoop => AARCH64_ARRAY_FILL_LOOP.to_owned(),
        AffineI32Loop => AARCH64_AFFINE_I32_LOOP.to_owned(),
        I32CounterLoop => AARCH64_I32_COUNTER_LOOP.to_owned(),
        BooleanReductionLoop => AARCH64_BOOLEAN_REDUCTION_LOOP.to_owned(),
        BranchRecurrenceLoop => AARCH64_BRANCH_RECURRENCE_LOOP.to_owned(),
        NestedXorLoop => AARCH64_NESTED_XOR_LOOP.to_owned(),
        SwitchReductionLoop => AARCH64_SWITCH_REDUCTION_LOOP.to_owned(),
        NumericIntegerLoop => AARCH64_NUMERIC_INTEGER_LOOP.to_owned(),
        NumericFloatingLoop => AARCH64_NUMERIC_FLOATING_LOOP.to_owned(),
        NumericBitwiseLoop => AARCH64_NUMERIC_BITWISE_LOOP.to_owned(),
        NumericIndependentLoop => AARCH64_NUMERIC_INDEPENDENT_LOOP.to_owned(),
        NumericMixedLoop => AARCH64_NUMERIC_MIXED_LOOP.to_owned(),
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
