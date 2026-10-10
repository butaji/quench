//! Wasm opcodes of the shared vocabulary. The general step delegates the
//! whole Wasm family here, so JavaScript dispatch keeps its own compact match
//! while both run in the same loop over the same frames.

use super::*;
use crate::wasm::integer::I32BinaryOperator;

impl<H: Host> Vm<H> {
    #[inline(never)]
    pub(super) fn step_wasm(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: &mut usize,
    ) -> Result<StepResult, JsError> {
        match i.op() {
            Op::WasmIndirectTarget => {
                let table = self.read(f, i.register_b());
                let index = self.wasm_table_index(table, self.read(f, i.register_c()))?;
                let value =
                    self.wasm_indirect_target(self.frames[f].program, table, index, i.imm())?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmInstanceBinding => {
                let value = self
                    .heap
                    .environment_slot(self.frames[f].env, i.imm() as usize)
                    .ok_or_else(|| JsError::validation("invalid Wasm instance binding".into()))?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmGlobalGet => {
                let value = self.wasm_global_load(self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmGlobalSet => {
                self.wasm_global_store(self.read(f, i.register_b()), self.read(f, i.register_a()))?;
            }
            Op::WasmRefFunc => {
                let value = self.wasm_function_reference(p, i.imm(), self.frames[f].env)?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmRefIsNull => {
                let value = self.read(f, i.register_b()).is_null();
                self.write(f, i.result_register(), Value::integer(i32::from(value)));
            }
            Op::WasmRefAsNonNull => {
                let value = self.read(f, i.register_b());
                if value.is_null() {
                    return Err(JsError::wasm_trap_error(
                        crate::wasm::reference::NonNullCheck::from_tag(i.imm())
                            .expect("validated non-null check")
                            .trap(),
                    ));
                }
                self.write(f, i.result_register(), value);
            }
            Op::WasmTableSize => {
                let table = self.read(f, i.register_b());
                let size = self.wasm_table_elements(table)?.len() as u64;
                let value = self.encode_wasm_table_size(table, Some(size))?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmTableGet | Op::WasmTableSet => {
                let table = self.read(f, i.register_b());
                let index = self.wasm_table_index(table, self.read(f, i.register_c()))?;
                if i.op() == Op::WasmTableGet {
                    let value = self.wasm_table_get(table, index)?;
                    self.write(f, i.result_register(), value);
                } else {
                    self.wasm_table_fill(table, index, self.read(f, i.register_a()), 1)?;
                }
            }
            Op::WasmTableGrow => {
                let table = self.read(f, i.register_b());
                let (delta, _) = i.register_pair();
                let delta = self.wasm_table_index(table, self.read(f, delta))?;
                let size = self.wasm_table_grow(table, self.read(f, i.register_c()), delta)?;
                let value = self.encode_wasm_table_size(table, size)?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmTableFill | Op::WasmTableCopy | Op::WasmTableInit => {
                let table = self.read(f, i.register_a());
                let source = self.read(f, i.register_b());
                let (output, input) = i.register_pair();
                let output = self.wasm_table_index(table, self.read(f, output))?;
                let table_type = self.wasm_table_index_type(table)?;
                let length_type = match i.op() {
                    Op::WasmTableInit => crate::WasmType::I32,
                    Op::WasmTableCopy => match (table_type, self.wasm_table_index_type(source)?) {
                        (crate::WasmType::I64, crate::WasmType::I64) => crate::WasmType::I64,
                        _ => crate::WasmType::I32,
                    },
                    _ => table_type,
                };
                let length = self.wasm_index_operand(self.read(f, i.register_c()), length_type)?;
                match i.op() {
                    Op::WasmTableFill => self.wasm_table_fill(table, output, source, length)?,
                    Op::WasmTableInit => {
                        let input =
                            self.wasm_index_operand(self.read(f, input), crate::WasmType::I32)?;
                        self.wasm_table_init(table, source, output, input, length)?;
                    }
                    _ => {
                        let input = self.wasm_table_index(source, self.read(f, input))?;
                        self.wasm_table_copy(table, source, output, input, length)?;
                    }
                }
            }
            Op::WasmMemoryInit | Op::WasmMemoryCopy | Op::WasmMemoryFill => {
                let destination = self.read(f, i.register_a());
                let source = self.read(f, i.register_b());
                let (output, input) = i.register_pair();
                let output = self.wasm_memory_index(destination, self.read(f, output))?;
                let index_type = self.wasm_memory_index_type(destination)?;
                let length_type = match i.op() {
                    Op::WasmMemoryInit => crate::WasmType::I32,
                    Op::WasmMemoryCopy => {
                        match (index_type, self.wasm_memory_index_type(source)?) {
                            (crate::WasmType::I64, crate::WasmType::I64) => crate::WasmType::I64,
                            _ => crate::WasmType::I32,
                        }
                    }
                    _ => index_type,
                };
                let length = self.wasm_index_operand(self.read(f, i.register_c()), length_type)?;
                if i.op() == Op::WasmMemoryFill {
                    self.wasm_memory_fill(
                        destination,
                        output,
                        Self::wasm_u32_operand(source)?,
                        length,
                    )?;
                } else {
                    let input = if i.op() == Op::WasmMemoryInit {
                        self.wasm_index_operand(self.read(f, input), crate::WasmType::I32)?
                    } else {
                        self.wasm_memory_index(source, self.read(f, input))?
                    };
                    let source = if i.op() == Op::WasmMemoryInit && source == Value::UNDEFINED {
                        None
                    } else {
                        Some(source)
                    };
                    self.wasm_copy_bytes(destination, source, output, input, length)?;
                }
            }
            Op::WasmMemoryAddress => {
                let memory = self.read(f, i.register_c());
                let address = self.wasm_memory_index(memory, self.read(f, i.register_b()))?;
                let Constant::WasmBits64(offset) = p.constants[i.constant_index()] else {
                    return Err(JsError::validation(
                        "invalid Wasm memory offset constant".into(),
                    ));
                };
                let effective = address
                    .checked_add(offset)
                    .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsMemory))?;
                let value = self.encode_wasm_value(crate::WasmValue::I64(effective as i64));
                self.write(f, i.result_register(), value);
            }
            Op::WasmAtomicFence => crate::wasm::atomic::fence(),
            Op::WasmAtomicAccess => {
                use crate::wasm::atomic::{AtomicInput, AtomicOperator};
                let operator =
                    AtomicOperator::from_tag(i.imm()).expect("validated atomic operator");
                let window = i.register_window();
                let input = |role: AtomicInput| self.read(f, window.base + role as u16);
                let memory = input(AtomicInput::Memory);
                let address = self.wasm_i64_operand(input(AtomicInput::Address))? as u64;
                let value = if window.count > AtomicInput::Value as u16 {
                    Some(self.decode_wasm_value(input(AtomicInput::Value), operator.value_type())?)
                } else {
                    None
                };
                let replacement = if window.count > AtomicInput::Auxiliary as u16 {
                    Some(self.decode_wasm_value(
                        input(AtomicInput::Auxiliary),
                        operator.auxiliary_type(),
                    )?)
                } else {
                    None
                };
                let Some(Cell::WasmMemory { bytes, .. }) = self.heap.get(memory) else {
                    return Err(JsError::validation("invalid atomic memory binding".into()));
                };
                let result = operator
                    .apply(bytes, address, value, replacement)
                    .map_err(JsError::wasm_trap_error)?;
                let result = result
                    .map(|value| self.encode_wasm_value(value))
                    .unwrap_or(Value::UNDEFINED);
                self.write(f, i.result_register(), result);
            }
            Op::WasmI32Load
            | Op::WasmI64Load
            | Op::WasmF32Load
            | Op::WasmF64Load
            | Op::WasmI32Load8S
            | Op::WasmI32Load8U
            | Op::WasmI32Load16S
            | Op::WasmI32Load16U
            | Op::WasmI64Load8S
            | Op::WasmI64Load8U
            | Op::WasmI64Load16S
            | Op::WasmI64Load16U
            | Op::WasmI64Load32S
            | Op::WasmI64Load32U => self.wasm_direct_load(f, i)?,
            Op::WasmI32Store
            | Op::WasmI64Store
            | Op::WasmF32Store
            | Op::WasmF64Store
            | Op::WasmI32Store8
            | Op::WasmI32Store16
            | Op::WasmI64Store8
            | Op::WasmI64Store16
            | Op::WasmI64Store32 => self.wasm_direct_store(f, i)?,
            Op::WasmMemoryLoad | Op::WasmMemoryStore => {
                let memory = self.read(f, i.register_b());
                let address = self.wasm_i64_operand(self.read(f, i.register_c()))? as u64;
                if i.op() == Op::WasmMemoryLoad {
                    let operator = crate::wasm::memory::MemoryLoad::from_tag(i.imm())
                        .expect("validated memory load");
                    let value = operator
                        .read(&self.wasm_memory_bytes(memory)?, address)
                        .map_err(JsError::wasm_trap_error)?;
                    let value = self.encode_wasm_value(value);
                    self.write(f, i.result_register(), value);
                } else {
                    let operator = crate::wasm::memory::MemoryStore::from_tag(i.imm())
                        .expect("validated memory store");
                    self.wasm_memory_store(
                        memory,
                        address,
                        operator,
                        self.read(f, i.register_a()),
                    )?;
                }
            }
            Op::WasmMemorySize | Op::WasmMemoryGrow => {
                let memory = self.read(f, i.register_b());
                let pages = if i.op() == Op::WasmMemoryGrow {
                    let delta = self.wasm_memory_index(memory, self.read(f, i.register_c()))?;
                    self.wasm_memory_grow(memory, delta)?
                } else {
                    Some(self.wasm_memory_pages(memory)?)
                };
                let index_type = self.wasm_memory_index_type(memory)?;
                let value = self.encode_wasm_index_size(index_type, pages);
                self.write(f, i.result_register(), value);
            }
            Op::WasmSimd => {
                let (operator, lane) = crate::wasm::simd::SimdOperator::from_selector(i.imm())
                    .expect("validated SIMD operator");
                let left =
                    self.decode_wasm_value(self.read(f, i.register_b()), operator.left_type())?;
                let right = operator
                    .right_type()
                    .map(|ty| self.decode_wasm_value(self.read(f, i.register_c()), ty))
                    .transpose()?;
                let value = self.encode_wasm_value(operator.apply(left, right, lane));
                self.write(f, i.result_register(), value);
            }
            Op::WasmSimdShuffle => {
                let crate::WasmValue::V128(left) =
                    self.decode_wasm_value(self.read(f, i.register_b()), crate::WasmType::V128)?
                else {
                    unreachable!()
                };
                let crate::WasmValue::V128(right) =
                    self.decode_wasm_value(self.read(f, i.register_c()), crate::WasmType::V128)?
                else {
                    unreachable!()
                };
                let Constant::WasmV128(indices) = p.constants[i.constant_index()] else {
                    unreachable!("validated shuffle indices")
                };
                let bits = crate::wasm::simd::shuffle(left, right, indices);
                let value = self.encode_wasm_value(crate::WasmValue::V128(bits));
                self.write(f, i.result_register(), value);
            }
            Op::WasmUnreachable => {
                return Err(JsError::wasm_trap_error(crate::WasmTrap::Unreachable));
            }
            Op::WasmI32Add => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::Add, right)?;
            }
            Op::WasmI32Subtract => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::Subtract, right)?;
            }
            Op::WasmI32Multiply => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::Multiply, right)?;
            }
            Op::WasmI32DivideSigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::DivideSigned, right)?;
            }
            Op::WasmI32DivideUnsigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::DivideUnsigned, right)?;
            }
            Op::WasmI32RemainderSigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::RemainderSigned, right)?;
            }
            Op::WasmI32RemainderUnsigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::RemainderUnsigned, right)?;
            }
            Op::WasmI32And => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::And, right)?;
            }
            Op::WasmI32Or => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::Or, right)?;
            }
            Op::WasmI32Xor => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::Xor, right)?;
            }
            Op::WasmI32ShiftLeft => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::ShiftLeft, right)?;
            }
            Op::WasmI32ShiftRightSigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::ShiftRightSigned, right)?;
            }
            Op::WasmI32ShiftRightUnsigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::ShiftRightUnsigned, right)?;
            }
            Op::WasmI32RotateLeft => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::RotateLeft, right)?;
            }
            Op::WasmI32RotateRight => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::RotateRight, right)?;
            }
            Op::WasmI32Equal => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::Equal, right)?;
            }
            Op::WasmI32NotEqual => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::NotEqual, right)?;
            }
            Op::WasmI32LessSigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::LessSigned, right)?;
            }
            Op::WasmI32LessUnsigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::LessUnsigned, right)?;
            }
            Op::WasmI32GreaterSigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::GreaterSigned, right)?;
            }
            Op::WasmI32GreaterUnsigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::GreaterUnsigned, right)?;
            }
            Op::WasmI32LessEqualSigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::LessEqualSigned, right)?;
            }
            Op::WasmI32LessEqualUnsigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::LessEqualUnsigned, right)?;
            }
            Op::WasmI32GreaterEqualSigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::GreaterEqualSigned, right)?;
            }
            Op::WasmI32GreaterEqualUnsigned => {
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, I32BinaryOperator::GreaterEqualUnsigned, right)?;
            }
            Op::WasmI32AddImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::Add, i.imm() as i32)?;
            }
            Op::WasmI32SubtractImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::Subtract, i.imm() as i32)?;
            }
            Op::WasmI32MultiplyImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::Multiply, i.imm() as i32)?;
            }
            Op::WasmI32DivideSignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::DivideSigned, i.imm() as i32)?;
            }
            Op::WasmI32DivideUnsignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::DivideUnsigned, i.imm() as i32)?;
            }
            Op::WasmI32RemainderSignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::RemainderSigned, i.imm() as i32)?;
            }
            Op::WasmI32RemainderUnsignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::RemainderUnsigned, i.imm() as i32)?;
            }
            Op::WasmI32AndImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::And, i.imm() as i32)?;
            }
            Op::WasmI32OrImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::Or, i.imm() as i32)?;
            }
            Op::WasmI32XorImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::Xor, i.imm() as i32)?;
            }
            Op::WasmI32ShiftLeftImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::ShiftLeft, i.imm() as i32)?;
            }
            Op::WasmI32ShiftRightSignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::ShiftRightSigned, i.imm() as i32)?;
            }
            Op::WasmI32ShiftRightUnsignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::ShiftRightUnsigned, i.imm() as i32)?;
            }
            Op::WasmI32RotateLeftImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::RotateLeft, i.imm() as i32)?;
            }
            Op::WasmI32RotateRightImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::RotateRight, i.imm() as i32)?;
            }
            Op::WasmI32EqualImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::Equal, i.imm() as i32)?;
            }
            Op::WasmI32NotEqualImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::NotEqual, i.imm() as i32)?;
            }
            Op::WasmI32LessSignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::LessSigned, i.imm() as i32)?;
            }
            Op::WasmI32LessUnsignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::LessUnsigned, i.imm() as i32)?;
            }
            Op::WasmI32GreaterSignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::GreaterSigned, i.imm() as i32)?;
            }
            Op::WasmI32GreaterUnsignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::GreaterUnsigned, i.imm() as i32)?;
            }
            Op::WasmI32LessEqualSignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::LessEqualSigned, i.imm() as i32)?;
            }
            Op::WasmI32LessEqualUnsignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::LessEqualUnsigned, i.imm() as i32)?;
            }
            Op::WasmI32GreaterEqualSignedImmediate => {
                self.wasm_i32_binary(f, i, I32BinaryOperator::GreaterEqualSigned, i.imm() as i32)?;
            }
            Op::WasmI32GreaterEqualUnsignedImmediate => {
                self.wasm_i32_binary(
                    f,
                    i,
                    I32BinaryOperator::GreaterEqualUnsigned,
                    i.imm() as i32,
                )?;
            }
            Op::WasmJumpI32Equal => {
                self.wasm_i32_jump(p, f, i, pc, I32BinaryOperator::Equal)?;
            }
            Op::WasmJumpI32NotEqual => {
                self.wasm_i32_jump(p, f, i, pc, I32BinaryOperator::NotEqual)?;
            }
            Op::WasmJumpI32LessSigned => {
                self.wasm_i32_jump(p, f, i, pc, I32BinaryOperator::LessSigned)?;
            }
            Op::WasmJumpI32LessUnsigned => {
                self.wasm_i32_jump(p, f, i, pc, I32BinaryOperator::LessUnsigned)?;
            }
            Op::WasmJumpI32GreaterSigned => {
                self.wasm_i32_jump(p, f, i, pc, I32BinaryOperator::GreaterSigned)?;
            }
            Op::WasmJumpI32GreaterUnsigned => {
                self.wasm_i32_jump(p, f, i, pc, I32BinaryOperator::GreaterUnsigned)?;
            }
            Op::WasmJumpI32LessEqualSigned => {
                self.wasm_i32_jump(p, f, i, pc, I32BinaryOperator::LessEqualSigned)?;
            }
            Op::WasmJumpI32LessEqualUnsigned => {
                self.wasm_i32_jump(p, f, i, pc, I32BinaryOperator::LessEqualUnsigned)?;
            }
            Op::WasmJumpI32GreaterEqualSigned => {
                self.wasm_i32_jump(p, f, i, pc, I32BinaryOperator::GreaterEqualSigned)?;
            }
            Op::WasmJumpI32GreaterEqualUnsigned => {
                self.wasm_i32_jump(p, f, i, pc, I32BinaryOperator::GreaterEqualUnsigned)?;
            }
            Op::WasmFillRegisters => {
                let value = self
                    .programs
                    .constant(self.frames[f].program, i.constant_index())
                    .ok_or_else(|| {
                        JsError::validation("constant index is outside program".into())
                    })?;
                let window = i.register_window();
                for offset in 0..window.count {
                    self.write(f, window.base + offset, value);
                }
            }
            Op::WasmSelect => {
                let (condition, _) = i.register_pair();
                let source = if self.read(f, condition).wasm_bits32() != 0 {
                    i.register_b()
                } else {
                    i.register_c()
                };
                self.write(f, i.result_register(), self.read(f, source));
            }
            Op::WasmJumpI32EqualImmediate => {
                self.wasm_i32_constant_jump(p, f, i, pc, I32BinaryOperator::Equal)?;
            }
            Op::WasmJumpI32NotEqualImmediate => {
                self.wasm_i32_constant_jump(p, f, i, pc, I32BinaryOperator::NotEqual)?;
            }
            Op::WasmJumpI32LessSignedImmediate => {
                self.wasm_i32_constant_jump(p, f, i, pc, I32BinaryOperator::LessSigned)?;
            }
            Op::WasmJumpI32LessUnsignedImmediate => {
                self.wasm_i32_constant_jump(p, f, i, pc, I32BinaryOperator::LessUnsigned)?;
            }
            Op::WasmJumpI32GreaterSignedImmediate => {
                self.wasm_i32_constant_jump(p, f, i, pc, I32BinaryOperator::GreaterSigned)?;
            }
            Op::WasmJumpI32GreaterUnsignedImmediate => {
                self.wasm_i32_constant_jump(p, f, i, pc, I32BinaryOperator::GreaterUnsigned)?;
            }
            Op::WasmJumpI32LessEqualSignedImmediate => {
                self.wasm_i32_constant_jump(p, f, i, pc, I32BinaryOperator::LessEqualSigned)?;
            }
            Op::WasmJumpI32LessEqualUnsignedImmediate => {
                self.wasm_i32_constant_jump(p, f, i, pc, I32BinaryOperator::LessEqualUnsigned)?;
            }
            Op::WasmJumpI32GreaterEqualSignedImmediate => {
                self.wasm_i32_constant_jump(p, f, i, pc, I32BinaryOperator::GreaterEqualSigned)?;
            }
            Op::WasmJumpI32GreaterEqualUnsignedImmediate => {
                self.wasm_i32_constant_jump(p, f, i, pc, I32BinaryOperator::GreaterEqualUnsigned)?;
            }
            Op::WasmBranchTable => {
                // `pc` already names the first entry; out-of-range selectors
                // take the default entry after the cases.
                let case = self.read(f, i.register_a()).wasm_bits32().min(i.imm());
                *pc += case as usize;
            }
            Op::WasmJumpI32Zero => {
                let taken = self.read(f, i.register_a()).wasm_bits32() == 0;
                self.wasm_jump(p, f, i, pc, taken);
            }
            Op::WasmJumpI32NonZero => {
                let taken = self.read(f, i.register_a()).wasm_bits32() != 0;
                self.wasm_jump(p, f, i, pc, taken);
            }
            Op::WasmArrayGet | Op::WasmArrayGetS | Op::WasmArrayGetU | Op::WasmArraySet => {
                let access =
                    crate::wasm::gc::GcFieldAccess::from_op(i.op()).expect("array field opcode");
                let write = i.op() == Op::WasmArraySet;
                let reference = self.read(
                    f,
                    if write {
                        i.register_a()
                    } else {
                        i.register_b()
                    },
                );
                let index = self
                    .read(
                        f,
                        if write {
                            i.register_b()
                        } else {
                            i.register_c()
                        },
                    )
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid Wasm array index".into()))?
                    as u32;
                let value = if write {
                    Some(self.read(f, i.register_c()))
                } else {
                    None
                };
                let result = self.wasm_gc_field(
                    reference,
                    super::wasm_gc::GcFieldIndex::Array(index),
                    access,
                    value,
                )?;
                if !write {
                    self.write(f, i.result_register(), result);
                }
            }
            Op::WasmArrayNewData | Op::WasmArrayNewElem => {
                use crate::wasm::gc::{ArraySegmentInput, ArraySegmentKind};
                let base = i.register_b();
                let input =
                    Self::wasm_u32_operand(self.read(f, base + ArraySegmentInput::Offset as u16))?;
                let count =
                    Self::wasm_u32_operand(self.read(f, base + ArraySegmentInput::Count as u16))?;
                let source = self.read(f, base + ArraySegmentInput::Segment as u16);
                let declarations = self
                    .programs
                    .wasm_signatures(self.frames[f].program)
                    .ok_or_else(|| {
                        JsError::validation("array constructor requires module declarations".into())
                    })?
                    .declarations
                    .clone();
                let value = self.wasm_array_segment_new(
                    &declarations,
                    i.imm(),
                    ArraySegmentKind::from_op(i.op()).unwrap(),
                    source,
                    input,
                    count,
                )?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmArrayInitData | Op::WasmArrayInitElem => {
                let reference = self.read(f, i.register_a());
                let source = self.read(f, i.register_b());
                let count = Self::wasm_u32_operand(self.read(f, i.register_c()))?;
                let (output, input) = i.register_pair();
                let output = Self::wasm_u32_operand(self.read(f, output))?;
                let input = Self::wasm_u32_operand(self.read(f, input))?;
                self.wasm_array_segment_init(
                    reference,
                    crate::wasm::gc::ArraySegmentKind::from_op(i.op()).unwrap(),
                    source,
                    output,
                    input,
                    count,
                )?;
            }
            Op::WasmArrayFill | Op::WasmArrayCopy => {
                let destination = self.read(f, i.register_a());
                let source = self.read(f, i.register_b());
                let count = Self::wasm_u32_operand(self.read(f, i.register_c()))?;
                let (output, input) = i.register_pair();
                let output = Self::wasm_u32_operand(self.read(f, output))?;
                if i.op() == Op::WasmArrayFill {
                    self.wasm_array_fill(destination, output, source, count)?;
                } else {
                    let input = Self::wasm_u32_operand(self.read(f, input))?;
                    self.wasm_array_copy(destination, source, output, input, count)?;
                }
            }
            Op::WasmArrayLen => {
                let length = self.wasm_array_len(self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), Value::integer(length as i32));
            }
            Op::WasmArrayNew | Op::WasmArrayNewDefault | Op::WasmArrayNewFixed => {
                let declarations = self
                    .programs
                    .wasm_signatures(self.frames[f].program)
                    .ok_or_else(|| {
                        JsError::validation("Wasm array requires registered declarations".into())
                    })?
                    .declarations
                    .clone();
                let initialization = if i.op() == Op::WasmArrayNewFixed {
                    let mut values = Vec::new();
                    values
                        .try_reserve_exact(usize::from(i.register_window().count))
                        .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::ArrayTooLarge))?;
                    for offset in 0..i.register_window().count {
                        values.push(self.read(f, i.register_b() + offset));
                    }
                    super::wasm_gc::ArrayInitialization::Fixed(values)
                } else {
                    let count_register = if i.op() == Op::WasmArrayNew {
                        i.register_c()
                    } else {
                        i.register_b()
                    };
                    let count =
                        self.read(f, count_register).as_int().ok_or_else(|| {
                            JsError::validation("invalid Wasm array length".into())
                        })? as u32;
                    if i.op() == Op::WasmArrayNew {
                        super::wasm_gc::ArrayInitialization::Repeated {
                            count,
                            value: self.read(f, i.register_b()),
                        }
                    } else {
                        super::wasm_gc::ArrayInitialization::Default(count)
                    }
                };
                let value = self.wasm_array_new(&declarations, i.imm(), initialization)?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmStructGet | Op::WasmStructGetS | Op::WasmStructGetU | Op::WasmStructSet => {
                let access =
                    crate::wasm::gc::GcFieldAccess::from_op(i.op()).expect("struct field opcode");
                let write = i.op() == Op::WasmStructSet;
                let reference = self.read(
                    f,
                    if write {
                        i.register_a()
                    } else {
                        i.register_b()
                    },
                );
                let value = if write {
                    Some(self.read(f, i.register_b()))
                } else {
                    None
                };
                let result = self.wasm_gc_field(
                    reference,
                    super::wasm_gc::GcFieldIndex::Struct(i.imm()),
                    access,
                    value,
                )?;
                if !write {
                    self.write(f, i.result_register(), result);
                }
            }
            Op::WasmStructNew
            | Op::WasmStructNewDefault
            | Op::WasmStructNewDesc
            | Op::WasmStructNewDefaultDesc
            | Op::WasmRefGetDesc => {
                let declarations = self
                    .programs
                    .wasm_signatures(self.frames[f].program)
                    .ok_or_else(|| {
                        JsError::validation("Wasm struct requires registered declarations".into())
                    })?
                    .declarations
                    .clone();
                if i.op() == Op::WasmRefGetDesc {
                    let value = self.wasm_ref_get_desc(
                        &declarations,
                        i.imm(),
                        self.read(f, i.register_b()),
                    )?;
                    self.write(f, i.result_register(), value);
                } else {
                    let mode = crate::wasm::gc::StructConstruction::from_op(i.op()).unwrap();
                    let window =
                        (!mode.defaulted() || mode.described()).then(|| i.register_window());
                    let descriptor = if mode.described() {
                        let window = window.unwrap();
                        Some(self.read(f, window.base + window.count - 1))
                    } else {
                        None
                    };
                    let values = if mode.defaulted() {
                        None
                    } else {
                        let window = window.unwrap();
                        let count = window.count - u16::from(mode.described());
                        let mut values = Vec::new();
                        values.try_reserve_exact(usize::from(count)).map_err(|_| {
                            JsError::validation("Wasm struct allocation failed".into())
                        })?;
                        values.extend((0..count).map(|offset| self.read(f, window.base + offset)));
                        Some(values)
                    };
                    let value = self.wasm_struct_new(&declarations, i.imm(), values, descriptor)?;
                    self.write(f, i.result_register(), value);
                }
            }
            Op::WasmExternalConversion => {
                let conversion = crate::wasm::reference::ExternalConversion::from_tag(i.imm())
                    .expect("validated external conversion");
                let input = self.read(f, i.register_b());
                self.decode_wasm_value(input, conversion.input_type())?;
                let value = self.wasm_external_conversion(conversion, input);
                self.write(f, i.result_register(), value);
            }
            Op::WasmRefEq => {
                let equal = self.read(f, i.register_b()) == self.read(f, i.register_c());
                self.write(f, i.result_register(), Value::integer(i32::from(equal)));
            }
            Op::WasmDescriptorTest | Op::WasmDescriptorCast => {
                let target = crate::wasm::reference::ReferenceTarget::from_tag(i.imm())
                    .expect("validated reference target");
                let owner = self
                    .programs
                    .wasm_signatures(self.frames[f].program)
                    .ok_or_else(|| {
                        JsError::validation(
                            "descriptor cast requires registered declarations".into(),
                        )
                    })?
                    .declarations
                    .clone();
                let reference = self.read(f, i.register_b());
                let valid = self.wasm_descriptor_matches(
                    &owner,
                    target,
                    reference,
                    self.read(f, i.register_c()),
                )?;
                let result = if i.op() == Op::WasmDescriptorTest {
                    Value::integer(i32::from(valid))
                } else if valid {
                    reference
                } else {
                    return Err(JsError::wasm_trap_error(
                        crate::WasmTrap::DescriptorCastFailure,
                    ));
                };
                self.write(f, i.result_register(), result);
            }
            Op::WasmRefTest | Op::WasmRefCast => {
                let reference = self.read(f, i.register_b());
                let target = crate::wasm::reference::ReferenceTarget::from_tag(i.imm())
                    .and_then(|target| target.reference_type())
                    .expect("validated reference target");
                let pool = self
                    .programs
                    .wasm_signatures(self.frames[f].program)
                    .ok_or_else(|| {
                        JsError::validation(
                            "reference cast requires registered module declarations".into(),
                        )
                    })?;
                let ty = pool
                    .declarations
                    .callable_value_type(wasmparser::ValType::Ref(target))
                    .ok_or_else(|| {
                        JsError::validation("invalid reference cast declaration".into())
                    })?;
                let valid = self.wasm_reference_valid_in(reference, ty, Some(&pool.declarations));
                let result = if i.op() == Op::WasmRefTest {
                    Value::integer(i32::from(valid))
                } else if valid {
                    reference
                } else {
                    return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                };
                self.write(f, i.result_register(), result);
            }
            Op::WasmI31 => {
                let operator = crate::wasm::i31::I31Operator::from_tag(i.imm())
                    .expect("validated i31 operator");
                let value =
                    self.decode_wasm_value(self.read(f, i.register_b()), operator.input_type())?;
                let value = operator.apply(value).map_err(JsError::wasm_trap_error)?;
                let value = self.encode_wasm_value(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmI32Unary => {
                let value = self
                    .read(f, i.register_b())
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid Wasm i32 operand".into()))?;
                let operator = crate::wasm::integer::I32UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm unary operator");
                let crate::WasmValue::I32(value) = operator.apply(value) else {
                    unreachable!("i32 operator result")
                };
                self.write(f, i.result_register(), Value::integer(value));
            }
            Op::WasmI64Binary => {
                let left = self.wasm_i64_operand(self.read(f, i.register_b()))?;
                let right = self.wasm_i64_operand(self.read(f, i.register_c()))?;
                let operator = crate::wasm::integer::I64BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm binary operator");
                let value = operator
                    .apply(left, right)
                    .map_err(JsError::wasm_trap_error)?;
                let value = self.encode_wasm_value(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmI64Unary => {
                let value = self.wasm_i64_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::integer::I64UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm unary operator");
                let value = self.encode_wasm_value(operator.apply(value));
                self.write(f, i.result_register(), value);
            }
            Op::WasmScalarConvert => {
                let operator = crate::wasm::conversion::ScalarConversionOperator::from_tag(i.imm())
                    .expect("validated Wasm scalar conversion");
                let value =
                    self.decode_wasm_value(self.read(f, i.register_b()), operator.source_type())?;
                let value = operator.apply(value).map_err(JsError::wasm_trap_error)?;
                debug_assert_eq!(value.ty(), operator.result_type());
                let value = self.encode_wasm_value(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmF32Binary => {
                let left = self.wasm_f32_operand(self.read(f, i.register_b()))?;
                let right = self.wasm_f32_operand(self.read(f, i.register_c()))?;
                let operator = crate::wasm::float::F32BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.apply(left, right));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF32Unary => {
                let left = self.wasm_f32_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::float::F32UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.apply(left));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF64Binary => {
                let left = self.wasm_f64_operand(self.read(f, i.register_b()))?;
                let right = self.wasm_f64_operand(self.read(f, i.register_c()))?;
                let operator = crate::wasm::float::F64BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.apply(left, right));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF64Unary => {
                let left = self.wasm_f64_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::float::F64UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.apply(left));
                self.write(f, i.result_register(), value);
            }
            Op::WasmExceptionNew => {
                use crate::wasm::tag::ExceptionInput;
                let window = i.register_window();
                if window.count < ExceptionInput::MIN_COUNT {
                    return Err(JsError::validation("missing Wasm exception tag".into()));
                }
                let tag = self.read(f, window.base + ExceptionInput::Tag as u16);
                let payload = (ExceptionInput::Payload as u16..window.count)
                    .map(|offset| self.read(f, window.base + offset))
                    .collect();
                let value = self.wasm_exception_new(tag, payload)?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmExceptionMatch => {
                let matches = self.wasm_exception_matches(
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                )?;
                self.write(f, i.result_register(), Value::integer(i32::from(matches)));
            }
            Op::WasmExceptionPayload => {
                let value = self.wasm_exception_payload(self.read(f, i.register_b()), i.imm())?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmThrowRef => return Err(self.wasm_throw_ref(self.read(f, i.register_a()))),
            op => unreachable!("{op:?} is not a Wasm opcode"),
        }
        Ok(StepResult::Continue)
    }
}
