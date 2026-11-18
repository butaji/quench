//! Wasm opcodes of the shared vocabulary. The general step delegates the
//! whole Wasm family here, so JavaScript dispatch keeps its own compact match
//! while both run in the same loop over the same frames.

use super::*;
use crate::staged::wasm_simd::WasmSimd as _;
use crate::staged::wasm_simd_concrete::{ConcreteSimd, execute};
use crate::wasm::integer::I32BinaryOperator;

mod gc;
pub(super) use gc::{wasm_gc_allocates, wasm_gc_field_op, wasm_gc_reads_instance, wasm_gc_value_op, wasm_gc_value_ops};

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
            Op::WasmInstanceBinding | Op::WasmGlobalGet | Op::WasmGlobalSet => {
                self.wasm_instance_step(f, i)?;
            }
            op if wasm_gc_value_op(op) => {
                if let Some(value) = self.wasm_gc_general_step(self.frames[f].program, self.frames[f].env, i, |vm, register| vm.read(f, register))? {
                    self.write(f, i.result_register(), value);
                }
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
                let (kernel, _) =
                    crate::wasm::simd::SimdKernel::of(i.imm()).expect("validated SIMD operator");
                let left = self.decode_wasm_value(self.read(f, i.register_b()), kernel.left)?;
                let right = kernel
                    .right
                    .map(|ty| self.decode_wasm_value(self.read(f, i.register_c()), ty))
                    .transpose()?;
                let value = self.encode_wasm_value(execute(i.imm(), left, right));
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
                let bits = ConcreteSimd.vector_shuffle(indices, left, right);
                let value = self.encode_wasm_value(crate::WasmValue::V128(bits));
                self.write(f, i.result_register(), value);
            }
            Op::WasmUnreachable => {
                return Err(JsError::wasm_trap_error(crate::WasmTrap::Unreachable));
            }
            // Every i32 binary operator, register or immediate form: the operator is the
            // opcode's (`from_register_op` / `from_immediate_op`), as the lane handler reads it.
            op if I32BinaryOperator::from_register_op(op).is_some() => {
                let operator = I32BinaryOperator::from_register_op(op).expect("an i32 operator");
                let right = self.read(f, i.register_c()).wasm_bits32() as i32;
                self.wasm_i32_binary(f, i, operator, right)?;
            }
            op if I32BinaryOperator::from_immediate_op(op).is_some() => {
                let operator = I32BinaryOperator::from_immediate_op(op).expect("an i32 operator");
                self.wasm_i32_binary(f, i, operator, i.imm() as i32)?;
            }
            op if I32BinaryOperator::from_jump_op(op).is_some() => {
                let operator = I32BinaryOperator::from_jump_op(op).expect("an i32 comparison");
                self.wasm_i32_jump(p, f, i, pc, operator)?;
            }
            op if I32BinaryOperator::from_immediate_jump_op(op).is_some() => {
                let operator = I32BinaryOperator::from_immediate_jump_op(op).expect("an i32 comparison");
                self.wasm_i32_constant_jump(p, f, i, pc, operator)?;
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
            Op::WasmI32ShiftRightUnsignedAndImmediate => {
                let value = self.read(f, i.register_b()).wasm_bits32() as i32;
                let fields = crate::wasm::integer::shift_right_unsigned_and(
                    value,
                    i32::from(i.c() as i16),
                    i.imm() as i32,
                );
                self.write(f, i.result_register(), Value::integer(fields));
            }
            Op::WasmI32AddImmediateJumpNonZero => {
                let value = self.read(f, i.register_b()).wasm_bits32() as i32;
                let sum = value.wrapping_add(i32::from(i.c() as i16));
                self.write(f, i.result_register(), Value::integer(sum));
                self.wasm_jump(p, f, i, pc, sum != 0);
            }
            Op::WasmBranchTable => *pc += self.branch_table_case(f, i),
            Op::WasmJumpI32Zero => {
                let taken = self.read(f, i.register_a()).wasm_bits32() == 0;
                self.wasm_jump(p, f, i, pc, taken);
            }
            Op::WasmJumpI32NonZero => {
                let taken = self.read(f, i.register_a()).wasm_bits32() != 0;
                self.wasm_jump(p, f, i, pc, taken);
            }
            Op::WasmI32Unary => {
                let value = self
                    .read(f, i.register_b())
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid Wasm i32 operand".into()))?;
                let operator = crate::wasm::integer::I32UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm unary operator");
                self.write(f, i.result_register(), Value::integer(operator.evaluate(value)));
            }
            Op::WasmI64Binary | Op::WasmI64BinaryImmediate => {
                let left = self.wasm_i64_operand(self.read(f, i.register_b()))?;
                let right = if i.op() == Op::WasmI64BinaryImmediate {
                    i64::from(i.c() as i16)
                } else {
                    self.wasm_i64_operand(self.read(f, i.register_c()))?
                };
                let operator = crate::wasm::integer::I64BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm binary operator");
                let value = operator
                    .evaluate(left, right)
                    .map_err(JsError::wasm_trap_error)?;
                let value = self.encode_wasm_value(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmI64BinaryRaw | Op::WasmI64BinaryImmediateRaw => {
                // Raw registers hold i64 words (`compile::liveness::raw_webs`).
                let left = self.read(f, i.register_b()).to_bits() as i64;
                let right = if i.op() == Op::WasmI64BinaryImmediateRaw {
                    i64::from(i.c() as i16)
                } else {
                    self.read(f, i.register_c()).to_bits() as i64
                };
                let operator = crate::wasm::integer::I64BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm binary operator");
                let value = operator
                    .evaluate(left, right)
                    .map_err(JsError::wasm_trap_error)?;
                let value = self.encode_raw_wasm_value(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmI64UnaryRaw => {
                let value = self.read(f, i.register_b()).to_bits() as i64;
                let operator = crate::wasm::integer::I64UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm unary operator");
                let value = self.encode_raw_wasm_value(operator.evaluate(value));
                self.write(f, i.result_register(), value);
            }
            Op::WasmReturn64 => {
                let word = self.read(f, i.register_a()).to_bits();
                return Ok(StepResult::Return(self.encode_bits64(word)));
            }
            Op::WasmI64Unary => {
                let value = self.wasm_i64_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::integer::I64UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm unary operator");
                let value = self.encode_wasm_value(operator.evaluate(value));
                self.write(f, i.result_register(), value);
            }
            Op::WasmScalarConvert => {
                let operator = crate::wasm::conversion::ScalarConversionOperator::from_tag(i.imm())
                    .expect("validated Wasm scalar conversion");
                let value =
                    self.decode_wasm_value(self.read(f, i.register_b()), operator.source_type())?;
                let value = operator.evaluate(value).map_err(JsError::wasm_trap_error)?;
                debug_assert_eq!(value.ty(), operator.result_type());
                let value = self.encode_wasm_value(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmF32Binary => {
                let left = self.wasm_f32_operand(self.read(f, i.register_b()))?;
                let right = self.wasm_f32_operand(self.read(f, i.register_c()))?;
                let operator = crate::wasm::float::F32BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.evaluate(left, right));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF32Unary => {
                let left = self.wasm_f32_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::float::F32UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.evaluate(left));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF64Binary => {
                let left = self.wasm_f64_operand(self.read(f, i.register_b()))?;
                let right = self.wasm_f64_operand(self.read(f, i.register_c()))?;
                let operator = crate::wasm::float::F64BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.evaluate(left, right));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF64Unary => {
                let left = self.wasm_f64_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::float::F64UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.evaluate(left));
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
