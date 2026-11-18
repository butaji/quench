//! The host's side of the Wasm call-out rows (`staged::protocol_wasm`): each step runs the
//! interpreter's own helper for its opcode (`vm/dispatch_wasm`, `vm/dispatch_wasm/gc`), with the
//! same reads: the tier's values for the operand registers, the running frame's for the rest.
use super::*;
use crate::bytecode::Register;
use crate::staged::protocol_wasm::{WasmHostStep, operands};

/// The value of register `register` the helper reads: the tier's value where `register` is one of
/// the two operand registers, else the running frame's.
fn operand_reader<H: Host>(
    frame: usize,
    operands: [Option<Register>; 2],
    values: [Option<Value>; 2],
) -> impl Fn(&Vm<H>, Register) -> Value + Copy {
    move |vm: &Vm<H>, register: Register| {
        let given = (0..2).find(|&k| operands[k] == Some(register)).and_then(|k| values[k]);
        given.unwrap_or_else(|| vm.read(frame, register))
    }
}

impl<H: Host> Vm<H> {
    /// The Wasm `step` of its operand values (`first` is the first operand register's value,
    /// `second` the second's): the value it defines, if any.
    pub(super) fn wasm_host_step(&mut self, _p: &ResidualProgram, _site: super::call_out::CallSite, step: WasmHostStep,
        first: Option<Value>, second: Option<Value>) -> Result<Option<Value>, JsError> {
        let i = step.fields().instruction();
        let frame = self.own_frame();
        let (program, env) = (self.frames[frame].program, self.frames[frame].env);
        let read = operand_reader::<H>(frame, operands(i), [first, second]);
        match step {
            WasmHostStep::WasmArrayCopy(_) | WasmHostStep::WasmArrayFill(_) | WasmHostStep::WasmArrayGet(_) | WasmHostStep::WasmArrayGetS(_) | WasmHostStep::WasmArrayGetU(_)
            | WasmHostStep::WasmArrayLen(_) | WasmHostStep::WasmArrayNew(_) | WasmHostStep::WasmArrayNewDefault(_) | WasmHostStep::WasmArrayNewFixed(_)
            | WasmHostStep::WasmArraySet(_) | WasmHostStep::WasmStructGet(_) | WasmHostStep::WasmStructGetS(_) | WasmHostStep::WasmStructGetU(_) | WasmHostStep::WasmStructSet(_)
            | WasmHostStep::WasmStructNew(_) | WasmHostStep::WasmStructNewDefault(_) | WasmHostStep::WasmStructNewDefaultDesc(_) | WasmHostStep::WasmStructNewDesc(_)
            | WasmHostStep::WasmRefGetDesc(_) | WasmHostStep::WasmRefIsNull(_) | WasmHostStep::WasmRefAsNonNull(_) | WasmHostStep::WasmRefEq(_) | WasmHostStep::WasmRefTest(_)
            | WasmHostStep::WasmRefCast(_) | WasmHostStep::WasmDescriptorTest(_) | WasmHostStep::WasmDescriptorCast(_) | WasmHostStep::WasmI31(_)
            | WasmHostStep::WasmExternalConversion(_) | WasmHostStep::WasmArrayNewData(_) | WasmHostStep::WasmArrayNewElem(_)
            | WasmHostStep::WasmArrayInitData(_) | WasmHostStep::WasmArrayInitElem(_) => {
                self.wasm_gc_value_step(program, env, i, read)
            }
            WasmHostStep::WasmTableSize(_) => {
                let table = read(&*self, i.register_b());
                let size = self.wasm_table_elements(table)?.len() as u64;
                self.encode_wasm_table_size(table, Some(size)).map(Some)
            }
            WasmHostStep::WasmTableGet(_) => {
                let table = read(&*self, i.register_b());
                let index = self.wasm_table_index(table, read(&*self, i.register_c()))?;
                self.wasm_table_get(table, index).map(Some)
            }
            WasmHostStep::WasmTableSet(_) => {
                let table = read(&*self, i.register_b());
                let index = self.wasm_table_index(table, read(&*self, i.register_c()))?;
                let value = read(&*self, i.register_a());
                self.wasm_table_fill(table, index, value, 1)?;
                Ok(None)
            }
            WasmHostStep::WasmTableGrow(_) => {
                let table = read(&*self, i.register_b());
                let (delta, _) = i.register_pair();
                let delta = self.wasm_table_index(table, read(&*self, delta))?;
                let initial = read(&*self, i.register_c());
                let size = self.wasm_table_grow(table, initial, delta)?;
                self.encode_wasm_table_size(table, size).map(Some)
            }
            WasmHostStep::WasmTableFill(_) | WasmHostStep::WasmTableCopy(_) | WasmHostStep::WasmTableInit(_) => {
                let table = read(&*self, i.register_a());
                let source = read(&*self, i.register_b());
                let (output, input) = i.register_pair();
                let output = self.wasm_table_index(table, read(&*self, output))?;
                let table_type = self.wasm_table_index_type(table)?;
                let length_type = match step {
                    WasmHostStep::WasmTableInit(_) => crate::WasmType::I32,
                    WasmHostStep::WasmTableCopy(_) => match (table_type, self.wasm_table_index_type(source)?) {
                        (crate::WasmType::I64, crate::WasmType::I64) => crate::WasmType::I64,
                        _ => crate::WasmType::I32,
                    },
                    _ => table_type,
                };
                let length = self.wasm_index_operand(read(&*self, i.register_c()), length_type)?;
                match step {
                    WasmHostStep::WasmTableFill(_) => self.wasm_table_fill(table, output, source, length)?,
                    WasmHostStep::WasmTableInit(_) => {
                        let input = self.wasm_index_operand(read(&*self, input), crate::WasmType::I32)?;
                        self.wasm_table_init(table, source, output, input, length)?;
                    }
                    _ => {
                        let input = self.wasm_table_index(source, read(&*self, input))?;
                        self.wasm_table_copy(table, source, output, input, length)?;
                    }
                }
                Ok(None)
            }
            WasmHostStep::WasmMemoryInit(_) => {
                let destination = read(&*self, i.register_a());
                let source = read(&*self, i.register_b());
                let (output, input) = i.register_pair();
                let output = self.wasm_memory_index(destination, read(&*self, output))?;
                let length = self.wasm_index_operand(read(&*self, i.register_c()), crate::WasmType::I32)?;
                let input = self.wasm_index_operand(read(&*self, input), crate::WasmType::I32)?;
                let source = if source == Value::UNDEFINED { None } else { Some(source) };
                self.wasm_copy_bytes(destination, source, output, input, length)?;
                Ok(None)
            }
            WasmHostStep::WasmAtomicFence(_) => {
                crate::wasm::atomic::fence();
                Ok(None)
            }
            WasmHostStep::WasmAtomicAccess(_) => {
                use crate::wasm::atomic::{AtomicInput, AtomicOperator};
                let operator = AtomicOperator::from_tag(i.imm()).expect("validated atomic operator");
                let window = i.register_window();
                let register = |role: AtomicInput| window.base + role as u16;
                let memory = read(&*self, register(AtomicInput::Memory));
                let address = self.wasm_i64_operand(read(&*self, register(AtomicInput::Address)))? as u64;
                let value = if window.count > AtomicInput::Value as u16 {
                    let value = read(&*self, register(AtomicInput::Value));
                    Some(self.decode_wasm_value(value, operator.value_type())?)
                } else {
                    None
                };
                let replacement = if window.count > AtomicInput::Auxiliary as u16 {
                    let replacement = read(&*self, register(AtomicInput::Auxiliary));
                    Some(self.decode_wasm_value(replacement, operator.auxiliary_type())?)
                } else {
                    None
                };
                let Some(Cell::WasmMemory { bytes, .. }) = self.heap.get(memory) else {
                    return Err(JsError::validation("invalid atomic memory binding".into()));
                };
                let result = operator
                    .apply(bytes, address, value, replacement)
                    .map_err(JsError::wasm_trap_error)?;
                Ok(Some(result.map(|value| self.encode_wasm_value(value)).unwrap_or(Value::UNDEFINED)))
            }
            WasmHostStep::WasmExceptionNew(_) => {
                use crate::wasm::tag::ExceptionInput;
                let window = i.register_window();
                if window.count < ExceptionInput::MIN_COUNT {
                    return Err(JsError::validation("missing Wasm exception tag".into()));
                }
                let tag = read(&*self, window.base + ExceptionInput::Tag as u16);
                let payload: Vec<Value> = (ExceptionInput::Payload as u16..window.count)
                    .map(|offset| read(&*self, window.base + offset))
                    .collect();
                self.wasm_exception_new(tag, payload).map(Some)
            }
            WasmHostStep::WasmExceptionMatch(_) => {
                let exception = read(&*self, i.register_b());
                let expected = read(&*self, i.register_c());
                let matches = self.wasm_exception_matches(exception, expected)?;
                Ok(Some(Value::integer(i32::from(matches))))
            }
            WasmHostStep::WasmExceptionPayload(_) => {
                let exception = read(&*self, i.register_b());
                self.wasm_exception_payload(exception, i.imm()).map(Some)
            }
            WasmHostStep::WasmThrowRef(_) => {
                let exception = read(&*self, i.register_a());
                Err(self.wasm_throw_ref(exception))
            }
        }
    }
}
