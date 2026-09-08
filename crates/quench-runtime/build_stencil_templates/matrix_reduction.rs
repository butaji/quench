pub(super) const AARCH64: &str = r##"#![no_std]
use core::arch::global_asm;

#[repr(C)]
struct MatrixReductionContext {
    left_rows: *const *const f64,
    right_rows: *const *const f64,
    row: u32,
    rows: u32,
    column: u32,
    columns: u32,
    inner: u32,
    inner_length: u32,
    total: f64,
    interrupt: *const u8,
}

global_asm!(r#"
.text
.p2align 2
.globl q_matrix_reduction_loop
q_matrix_reduction_loop:
  ldr w1, [x0, #{row}]
  ldr w2, [x0, #{rows}]
  ldr w3, [x0, #{column}]
  ldr w4, [x0, #{inner}]
  ldr d0, [x0, #{total}]
1:
  cmp w1, w2
  b.ge 9f
  ldr w10, [x0, #{columns}]
  cmp w3, w10
  b.ge 7f
  ldr w10, [x0, #{inner_length}]
  cmp w4, w10
  b.ge 6f
  ldr x5, [x0, #{left_rows}]
  add x5, x5, x1, lsl #3
  ldr x6, [x5]
  add x6, x6, x4, lsl #3
  ldr d1, [x6]
  ldr x7, [x0, #{right_rows}]
  add x7, x7, x4, lsl #3
  ldr x8, [x7]
  add x8, x8, x3, lsl #3
  ldr d2, [x8]
  fmul d1, d1, d2
  fadd d0, d0, d1
  add w4, w4, #1
  str w4, [x0, #{inner}]
  str d0, [x0, #{total}]
  ldr x14, [x0, #{interrupt}]
  ldrb w15, [x14]
  cbnz w15, 10f
  b 1b
6:
  mov w4, #0
  str w4, [x0, #{inner}]
  add w3, w3, #1
  str w3, [x0, #{column}]
  b 1b
7:
  mov w3, #0
  str w3, [x0, #{column}]
  add w1, w1, #1
  str w1, [x0, #{row}]
  b 1b
9:
  str d0, [x0, #{total}]
  mov w0, #1
  ret
10:
  mov w0, #4
  ret
q_matrix_reduction_loop_end:
"#,
    left_rows = const core::mem::offset_of!(MatrixReductionContext, left_rows),
    right_rows = const core::mem::offset_of!(MatrixReductionContext, right_rows),
    row = const core::mem::offset_of!(MatrixReductionContext, row),
    rows = const core::mem::offset_of!(MatrixReductionContext, rows),
    column = const core::mem::offset_of!(MatrixReductionContext, column),
    columns = const core::mem::offset_of!(MatrixReductionContext, columns),
    inner = const core::mem::offset_of!(MatrixReductionContext, inner),
    inner_length = const core::mem::offset_of!(MatrixReductionContext, inner_length),
    total = const core::mem::offset_of!(MatrixReductionContext, total),
    interrupt = const core::mem::offset_of!(MatrixReductionContext, interrupt),
);
"##;
