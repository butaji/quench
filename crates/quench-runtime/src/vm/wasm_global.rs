//! The instance-binding and global steps of the Wasm interpreter and its lane handlers. A
//! global's access is the staged rule (`staged::wasm_instance`) on `ConcreteInstance` over the
//! global's native slot, which holds the raw payload of a number and the bits of a handle: the
//! tier reads and writes the same slot in place, and the interpreter converts between the slot
//! and its registers' encoding (`encode_bits64`) here, at the one boundary.

use super::*;
use crate::staged::wasm_instance::{Instance, Storage, WasmInstance};
use crate::staged::wasm_machine::ConcreteInstance;

/// The slot of the one native slot a global access's stage holds.
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
        self.heap.environment_slot(env, slot)
    }

    /// The register encoding of the word a global's slot holds under `storage`.
    fn wasm_slot_value(&mut self, storage: Storage, word: u64) -> Value {
        match storage {
            Storage::Bits32 => Value::integer(word as u32 as i32),
            Storage::Bits64 => self.encode_bits64(word),
            Storage::Handle => Value::from_bits(word),
        }
    }

    /// The word a global's slot holds under `storage` for the register encoding `value`.
    fn wasm_slot_word(&self, storage: Storage, value: Value) -> Option<u64> {
        match storage {
            Storage::Bits32 => Some(u64::from(value.wasm_bits32())),
            Storage::Bits64 => self.decode_bits64(value),
            Storage::Handle => Some(value.to_bits()),
        }
    }

    /// The word of the global of type `ty` that the register encoding `value` stores.
    pub(super) fn wasm_global_bits(&self, ty: crate::WasmType, value: Value) -> u64 {
        self.wasm_slot_word(Storage::of(ty), value)
            .expect("an encoded value of the global's type")
    }

    /// The value of the global bound to `binding`, as its register encoding: its slot's word
    /// under its type's storage.
    pub(super) fn wasm_global_load(&mut self, binding: Value) -> Result<Value, JsError> {
        let Some(Cell::WasmGlobal { bits, ty, .. }) = self.heap.get(binding) else {
            return Err(JsError::validation("invalid Wasm global binding".into()));
        };
        let (mut bits, storage) = (*bits, Storage::of(*ty));
        let word = ConcreteInstance {
            globals: std::slice::from_mut(&mut bits),
            memory: &mut [],
        }
        .global_load(CELL, storage);
        Ok(self.wasm_slot_value(storage, word))
    }

    /// The value of the global bound to `binding`.
    pub(super) fn wasm_global_read(&self, binding: Value) -> Result<crate::WasmValue, JsError> {
        let Some(Cell::WasmGlobal { bits, ty, header, .. }) = self.heap.get(binding) else {
            return Err(JsError::validation("invalid Wasm global binding".into()));
        };
        match Storage::of(*ty) {
            Storage::Handle => {
                self.decode_wasm_value_in(Value::from_bits(*bits), *ty, Some(&header.declarations))
            }
            Storage::Bits32 | Storage::Bits64 => Ok(crate::WasmValue::from_bits(*ty, *bits)),
        }
    }

    /// Stores `value`, a register encoding, in the mutable global bound to `binding`, as its
    /// type's storage holds it.
    pub(super) fn wasm_global_store(
        &mut self,
        binding: Value,
        value: Value,
    ) -> Result<(), JsError> {
        let Some(Cell::WasmGlobal {
            ty,
            mutable: true,
            ..
        }) = self.heap.get(binding)
        else {
            return Err(JsError::validation(
                "invalid or immutable Wasm global binding".into(),
            ));
        };
        let storage = Storage::of(*ty);
        let word = self
            .wasm_slot_word(storage, value)
            .ok_or_else(|| JsError::validation("invalid Wasm global value".into()))?;
        let Some(Cell::WasmGlobal { bits, .. }) = self.heap.get_mut(binding) else {
            unreachable!("the binding was a global");
        };
        ConcreteInstance {
            globals: std::slice::from_mut(bits),
            memory: &mut [],
        }
        .global_store(CELL, storage, word);
        Ok(())
    }
}
