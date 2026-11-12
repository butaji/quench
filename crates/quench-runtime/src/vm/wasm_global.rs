//! The instance-binding and global steps of the Wasm interpreter and its lane handlers. A
//! binding or a global's access is the staged rule (`staged::wasm_instance`) on
//! `ConcreteInstance` over the cell, with the storage its type decides, so the interpreter,
//! the lane and the tier move the same words.

use super::*;
use crate::staged::wasm_instance::{Instance, Storage, WasmInstance};
use crate::staged::wasm_machine::{ConcreteInstance, slot_value, slot_word};

/// The slot of the one cell a global access's stage holds.
const CELL: u32 = 0;

impl<H: Host> Vm<H> {
    /// An instance-binding or global step: the register `a` takes the environment slot `imm`, the
    /// global bound in `b`, or the global bound in `b` takes `a`.
    pub(super) fn wasm_instance_step(
        &mut self,
        f: usize,
        i: WideInstruction,
    ) -> Result<(), JsError> {
        match Instance::of(i.op()) {
            Some(Instance::Binding) => {
                let value = self
                    .wasm_binding(self.frames[f].env, i.imm() as usize)
                    .ok_or_else(|| JsError::validation("invalid Wasm instance binding".into()))?;
                self.write(f, i.result_register(), value);
            }
            Some(Instance::GlobalGet) => {
                let value = self.wasm_global_load(self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Some(Instance::GlobalSet) => {
                self.wasm_global_store(self.read(f, i.register_b()), self.read(f, i.register_a()))?;
            }
            _ => unreachable!("step_wasm routes the instance opcodes"),
        }
        Ok(())
    }

    /// The instance field at environment slot `slot` of `env`: a binding is a handle read from
    /// the environment's cell.
    pub(super) fn wasm_binding(&self, env: Value, slot: usize) -> Option<Value> {
        let field = self.heap.environment_slot(env, slot)?;
        Some(read_cell(field, Storage::Handle))
    }

    /// The value of the global bound to `binding`: its cell's word under its type's storage.
    pub(super) fn wasm_global_load(&self, binding: Value) -> Result<Value, JsError> {
        let Some(Cell::WasmGlobal { value, ty, .. }) = self.heap.get(binding) else {
            return Err(JsError::validation("invalid Wasm global binding".into()));
        };
        Ok(read_cell(*value, Storage::of(*ty)))
    }

    /// Stores `value` in the mutable global bound to `binding`, as its type's storage holds it.
    pub(super) fn wasm_global_store(
        &mut self,
        binding: Value,
        value: Value,
    ) -> Result<(), JsError> {
        let Some(Cell::WasmGlobal {
            value: cell,
            ty,
            mutable: true,
            ..
        }) = self.heap.get_mut(binding)
        else {
            return Err(JsError::validation(
                "invalid or immutable Wasm global binding".into(),
            ));
        };
        let storage = Storage::of(*ty);
        ConcreteInstance {
            globals: std::slice::from_mut(cell),
            memory: &mut [],
        }
        .global_store(CELL, storage, slot_word(storage, value));
        Ok(())
    }
}

/// The slot a register takes from the cell `cell` read as `storage`.
fn read_cell(mut cell: Value, storage: Storage) -> Value {
    let mut stage = ConcreteInstance {
        globals: std::slice::from_mut(&mut cell),
        memory: &mut [],
    };
    slot_value(storage, stage.global_load(CELL, storage))
}
