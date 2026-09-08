//! AArch64 physical forms for a verified Number DAG.

use super::{BinaryOp, NumericDagSelection, ValueRecipe, MAX_DAG_VALUES};

const FADD_D_BASE: u32 = 0x1e60_2800;
const FSUB_D_BASE: u32 = 0x1e60_3800;
const FMUL_D_BASE: u32 = 0x1e60_0800;
const FDIV_D_BASE: u32 = 0x1e60_1800;
const FMOV_D_BASE: u32 = 0x1e60_4000;
const LDR_D_LITERAL_BASE: u32 = 0x5c00_0000;
const RETURN: u32 = 0xd65f_03c0;
const NOP: u32 = 0xd503_201f;
const REGISTER_MASK: u32 = 0x1f;
const LITERAL_IMMEDIATE_MASK: u32 = 0x7_ffff;
const INSTRUCTION_BYTES: usize = 4;
const LITERAL_BYTES: usize = 8;
const LITERAL_ALIGNMENT: usize = 8;
const FIRST_SCRATCH: usize = 3;
const CALLER_SAVED_FP: [u8; 21] = [
    3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
];

struct LiteralUse {
    instruction: usize,
    register: u8,
    bits: u64,
}

pub(super) fn render(selection: NumericDagSelection) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut physical = [u8::MAX; MAX_DAG_VALUES];
    let mut literals = Vec::new();
    let mut scratch = 0usize;
    for (index, node) in selection.nodes[..usize::from(selection.len)]
        .iter()
        .enumerate()
    {
        physical[index] = emit_node(
            node.recipe,
            &physical,
            &mut bytes,
            &mut literals,
            &mut scratch,
        )?;
    }
    emit_word(
        &mut bytes,
        fmov(0, physical[usize::from(selection.result)])?,
    );
    emit_word(&mut bytes, RETURN);
    align_literals(&mut bytes);
    patch_literals(&mut bytes, literals)?;
    Some(bytes)
}

fn emit_node(
    recipe: ValueRecipe,
    physical: &[u8; MAX_DAG_VALUES],
    bytes: &mut Vec<u8>,
    literals: &mut Vec<LiteralUse>,
    scratch: &mut usize,
) -> Option<u8> {
    match recipe {
        ValueRecipe::Input(input) => (usize::from(input) < FIRST_SCRATCH).then_some(input),
        ValueRecipe::Alias(source) => physical.get(usize::from(source)).copied(),
        ValueRecipe::Constant(bits) => emit_literal(bytes, literals, bits, scratch),
        ValueRecipe::Binary { operator, lhs, rhs } => {
            emit_binary_node(operator, lhs, rhs, physical, bytes, scratch)
        }
        ValueRecipe::BinaryConstant {
            operator,
            source,
            bits,
            left,
        } => emit_binary_constant(
            operator, source, bits, left, physical, bytes, literals, scratch,
        ),
        ValueRecipe::Empty => None,
    }
}

fn emit_binary_node(
    operator: BinaryOp,
    lhs: u8,
    rhs: u8,
    physical: &[u8; MAX_DAG_VALUES],
    bytes: &mut Vec<u8>,
    scratch: &mut usize,
) -> Option<u8> {
    let dst = take_scratch(scratch)?;
    emit_binary(
        bytes,
        operator,
        dst,
        physical[usize::from(lhs)],
        physical[usize::from(rhs)],
    )?;
    Some(dst)
}

#[allow(clippy::too_many_arguments)]
fn emit_binary_constant(
    operator: BinaryOp,
    source: u8,
    bits: u64,
    left: bool,
    physical: &[u8; MAX_DAG_VALUES],
    bytes: &mut Vec<u8>,
    literals: &mut Vec<LiteralUse>,
    scratch: &mut usize,
) -> Option<u8> {
    let constant = emit_literal(bytes, literals, bits, scratch)?;
    let dst = take_scratch(scratch)?;
    let source = physical[usize::from(source)];
    let (lhs, rhs) = if left {
        (constant, source)
    } else {
        (source, constant)
    };
    emit_binary(bytes, operator, dst, lhs, rhs)?;
    Some(dst)
}

fn take_scratch(next: &mut usize) -> Option<u8> {
    let register = *CALLER_SAVED_FP.get(*next)?;
    *next += 1;
    Some(register)
}

fn emit_literal(
    bytes: &mut Vec<u8>,
    literals: &mut Vec<LiteralUse>,
    bits: u64,
    scratch: &mut usize,
) -> Option<u8> {
    let register = take_scratch(scratch)?;
    let instruction = bytes.len();
    emit_word(bytes, ldr_literal(register, 0)?);
    literals.push(LiteralUse {
        instruction,
        register,
        bits,
    });
    Some(register)
}

fn emit_binary(bytes: &mut Vec<u8>, operator: BinaryOp, dst: u8, lhs: u8, rhs: u8) -> Option<()> {
    let base = match operator {
        BinaryOp::Add => FADD_D_BASE,
        BinaryOp::Subtract => FSUB_D_BASE,
        BinaryOp::Multiply => FMUL_D_BASE,
        BinaryOp::Divide => FDIV_D_BASE,
        _ => return None,
    };
    emit_word(bytes, three_registers(base, dst, lhs, rhs)?);
    Some(())
}

fn three_registers(base: u32, dst: u8, lhs: u8, rhs: u8) -> Option<u32> {
    valid_registers(&[dst, lhs, rhs])
        .then(|| base | (u32::from(rhs) << 16) | (u32::from(lhs) << 5) | u32::from(dst))
}

fn fmov(dst: u8, source: u8) -> Option<u32> {
    valid_registers(&[dst, source]).then(|| FMOV_D_BASE | (u32::from(source) << 5) | u32::from(dst))
}

fn ldr_literal(register: u8, byte_offset: i32) -> Option<u32> {
    (u32::from(register) <= REGISTER_MASK && byte_offset % INSTRUCTION_BYTES as i32 == 0).then(
        || {
            let words = byte_offset / INSTRUCTION_BYTES as i32;
            LDR_D_LITERAL_BASE
                | ((words as u32 & LITERAL_IMMEDIATE_MASK) << 5)
                | u32::from(register)
        },
    )
}

fn valid_registers(registers: &[u8]) -> bool {
    registers
        .iter()
        .all(|register| u32::from(*register) <= REGISTER_MASK)
}

fn align_literals(bytes: &mut Vec<u8>) {
    while bytes.len() % LITERAL_ALIGNMENT != 0 {
        emit_word(bytes, NOP);
    }
}

fn patch_literals(bytes: &mut Vec<u8>, literals: Vec<LiteralUse>) -> Option<()> {
    for literal in literals {
        let target = bytes.len();
        bytes.extend_from_slice(&literal.bits.to_le_bytes());
        let displacement = i32::try_from(target.checked_sub(literal.instruction)?).ok()?;
        let word = ldr_literal(literal.register, displacement)?;
        bytes[literal.instruction..literal.instruction + INSTRUCTION_BYTES]
            .copy_from_slice(&word.to_le_bytes());
    }
    (bytes.len() % LITERAL_BYTES == 0).then_some(())
}

fn emit_word(bytes: &mut Vec<u8>, word: u32) {
    bytes.extend_from_slice(&word.to_le_bytes());
}
