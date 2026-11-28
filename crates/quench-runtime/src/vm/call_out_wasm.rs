//! The host's side of the Wasm call-out rows (`staged::protocol_wasm`): each step runs the
//! interpreter's own helper for its opcode (`vm/dispatch_wasm`, `vm/dispatch_wasm/gc`), with the
//! same reads: the tier's values for the operand registers, the running frame's for the rest.
use super::*;
use crate::bytecode::Register;
use crate::staged::protocol_wasm::{WasmHostFieldsProtocolWasmExt, WasmHostStep, operands};
use super::wasm_table::copy_length_type;

/// The value of register `register` the helper reads: the tier's value where `register` is one of
/// the two operand registers, else the running frame's.
fn operand_reader(
    frame: usize,
    operands: [Option<Register>; 2],
    values: [Option<Value>; 2],
) -> impl Fn(&Vm, Register) -> Value + Copy {
    move |vm: &Vm, register: Register| {
        let given = (0..2).find(|&k| operands[k] == Some(register)).and_then(|k| values[k]);
        given.unwrap_or_else(|| vm.read(frame, register))
    }
}

impl Vm {
    /// The Wasm `step` of its operand values (`first` is the first operand register's value,
    /// `second` the second's): the value it defines, if any.
    pub(super) fn wasm_host_step(&mut self, _p: &ProgramRecord, _site: super::call_out::CallSite, step: WasmHostStep,
        first: Option<Value>, second: Option<Value>) -> Result<Option<Value>, JsError> {
        let i = step.fields().instruction();
        let frame = self.own_frame();
        let (program, env) = (self.frames[frame].program, self.frames[frame].env);
        let read = operand_reader(frame, operands(i), [first, second]);
        match step {
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
                let table_type = self.wasm_table_index_type(table)?;
                let output = self.wasm_index_operand(read(&*self, output), table_type)?;
                // Each operand's index type is derived once: a copy's source is a table too.
                let (length_type, input_type) = match step {
                    WasmHostStep::WasmTableInit(_) => (crate::WasmType::I32, crate::WasmType::I32),
                    WasmHostStep::WasmTableCopy(_) => {
                        let source_type = self.wasm_table_index_type(source)?;
                        (copy_length_type(table_type, source_type), source_type)
                    }
                    _ => (table_type, table_type),
                };
                let length = self.wasm_index_operand(read(&*self, i.register_c()), length_type)?;
                match step {
                    WasmHostStep::WasmTableFill(_) => self.wasm_table_fill(table, output, source, length)?,
                    WasmHostStep::WasmTableInit(_) => {
                        let input = self.wasm_index_operand(read(&*self, input), input_type)?;
                        self.wasm_table_init(table, source, output, input, length)?;
                    }
                    _ => {
                        let input = self.wasm_index_operand(read(&*self, input), input_type)?;
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
            WasmHostStep::WasmAtomicAccess(_) => self.wasm_atomic_result(i, read).map(Some),
            WasmHostStep::WasmExceptionNew(_) => self.wasm_exception_from_window(i, read).map(Some),
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
            // Every other host step is a GC value step or an instance read (`gc_value_op`).
            _ => {
                debug_assert!(crate::staged::wasm_gc::gc_value_op(i.op()), "{:?} is no GC value step", i.op());
                self.wasm_gc_value_step(program, env, i, read)
            }
        }
    }
}
