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

/// One environment slot of an instance of a module, by the declaration it binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmBinding {
    Global(usize),
    Memory(usize),
    Data(usize),
    Table(usize),
    Element(usize),
    Tag(usize),
    /// The instance's record (`install_wasm_instance`).
    Instance,
}

/// How many declarations of each kind an instance binds: the environment's layout. Slots run globals
/// (imported and defined), memories, data segments, tables, elements, tags, then the instance's record;
/// the imported functions' slots follow it. The one layout instantiation builds
/// (`Vm::instantiate_wasm_module_with_imports`), lowering names (`Lowering::instance_binding`) and the
/// plan reads (`binding_types`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WasmLayout {
    pub globals: usize,
    pub memories: usize,
    pub data: usize,
    pub tables: usize,
    pub elements: usize,
    pub tags: usize,
}

impl WasmLayout {
    /// Each kind in slot order, with how it names a declaration.
    const ORDER: [fn(usize) -> WasmBinding; 7] =
        [WasmBinding::Global, WasmBinding::Memory, WasmBinding::Data, WasmBinding::Table, WasmBinding::Element, WasmBinding::Tag, |_| WasmBinding::Instance];

    fn counts(self) -> [usize; 7] {
        [self.globals, self.memories, self.data, self.tables, self.elements, self.tags, 1]
    }

    /// Every slot's binding, in slot order.
    pub fn bindings(self) -> impl Iterator<Item = WasmBinding> {
        Self::ORDER.into_iter().zip(self.counts()).flat_map(|(kind, count)| (0..count).map(kind))
    }

    /// The environment slot of `binding`.
    pub fn slot(self, binding: WasmBinding) -> usize {
        let (kind, index) = match binding {
            WasmBinding::Global(index) => (0, index),
            WasmBinding::Memory(index) => (1, index),
            WasmBinding::Data(index) => (2, index),
            WasmBinding::Table(index) => (3, index),
            WasmBinding::Element(index) => (4, index),
            WasmBinding::Tag(index) => (5, index),
            WasmBinding::Instance => (6, 0),
        };
        debug_assert_eq!(Self::ORDER[kind](index), binding);
        self.counts()[..kind].iter().sum::<usize>() + index
    }
}

impl WasmModule {
    /// The layout of an instance's environment (`WasmLayout`).
    pub fn layout(&self) -> WasmLayout {
        super::ModuleBindings {
            name: "", // the layout reads only the counts
            globals: &self.globals,
            memories: &self.memories,
            data: &self.data,
            tables: &self.tables,
            elements: &self.elements,
            tags: &self.tags,
        }.layout()
    }

    /// The binding types of an instance of this module, by environment slot (`WasmLayout`). An
    /// imported memory or table has its declared type; a shared memory is direct only over a reservation
    /// (`addressable_in_place`), which no declaration promises.
    pub fn binding_types(&self) -> Vec<WasmBindingType> {
        self.layout().bindings()
            .map(|binding| match binding {
                WasmBinding::Global(index) => WasmGlobal::value_type(&self.globals, index).map_or(WasmBindingType::Other, WasmBindingType::Global),
                WasmBinding::Memory(index) => {
                    let ty = self.memories[index].ty;
                    WasmBindingType::Memory { ty, direct: super::memory::addressable_in_place(ty.shared, false) }
                }
                WasmBinding::Table(index) => WasmBindingType::Table { table64: self.tables[index].ty.table64 },
                WasmBinding::Data(_) | WasmBinding::Element(_) | WasmBinding::Tag(_) | WasmBinding::Instance => WasmBindingType::Other,
            })
            .collect()
    }
}
