//! What a module's instance binds at each environment slot, as the types its declarations give: the
//! one source the Wasm plan reads (`Analysis::binding`), whether an instance answers it (from its cells)
//! or the module alone does (ahead of any instance, for an image).

use super::{WasmGlobal, WasmModule, WasmType};

/// The type of one environment binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmBindingType {
    Global(WasmType),
    /// A memory: its type, and whether code may address its bytes in place (`MemoryStorage::is_direct`).
    Memory { ty: wasmparser::MemoryType, direct: bool },
    Table { table64: bool },
    /// Anything the plan reads nothing of (a data segment, an element, a tag, the instance, a function).
    Other,
}

impl WasmModule {
    /// The binding types of an instance of this module, by environment slot, in the order
    /// `Vm::instantiate_wasm_module_with_imports` lays the environment out: globals, memories, data
    /// segments, tables, elements, tags, then the rest. An imported memory or table has its declared type;
    /// a shared memory counts as not direct, which no instance contradicts unsafely.
    pub fn binding_types(&self) -> Vec<WasmBindingType> {
        let globals = (0..self.globals.len())
            .map(|index| WasmGlobal::value_type(&self.globals, index).map_or(WasmBindingType::Other, WasmBindingType::Global));
        let memories = self.memories.iter().map(|memory| WasmBindingType::Memory { ty: memory.ty, direct: !memory.ty.shared });
        let data = self.data.iter().map(|_| WasmBindingType::Other);
        let tables = self.tables.iter().map(|table| WasmBindingType::Table { table64: table.ty.table64 });
        globals.chain(memories).chain(data).chain(tables).collect()
    }
}
