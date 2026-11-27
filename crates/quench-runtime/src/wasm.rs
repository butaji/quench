//! Wasm operator lowering into the shared residual vocabulary. Binary decoding
//! and module validation belong to quench-wasm, not this execution core.

use crate::bytecode::{
    AtomTable, Constant, Function, ImmediateLayout, Instr, Op, ProgramKind, Register,
    ResidualProgram, WideInstruction,
};
use crate::staged::wasm_emit::{self, Emission, SelectorFamily};
use crate::{Diagnostic, Engine};
use std::rc::Rc;
use wasmparser::{BinaryReaderError, Operator};

mod constant_pool;
use constant_pool::ConstantPool;
mod constant;
pub(crate) mod extern_kind;
pub mod gc;
pub use constant::WasmConstantExpression;
pub(crate) use constant::{WasmArrayInitializer, WasmInitializerEffect};
pub mod tag;
pub use tag::WasmTag;
mod element;
pub use element::{WasmElement, WasmElementItems, WasmElementMode};
pub use table::{WasmTable, WasmTableInitializer};
pub(crate) mod atomic;
pub mod memory;
mod bindings;
pub use bindings::{WasmBinding, WasmBindingType, WasmLayout};
mod wait;
pub const INDEX_GROW_FAILURE: i64 = -1;
pub use memory::{WasmData, WasmDataMode, WasmMemory};
mod control;
mod inline;
#[cfg(test)]
pub(crate) use inline::COMPILED_CALLS_KEPT_FOR_TEST;
mod scalar;
mod scalar_types;
mod memory_ops;
mod types;
pub use types::WasmTypes;
pub mod simd;
pub(crate) mod table;
pub use scalar::{CanonicalSignature, ScalarBits, V128_BYTES, WasmSignatures};
pub use scalar::{
    WasmHostSignature,
    WasmCallableType, WasmFunctionBody, WasmFunctionImport, WasmReferenceKind, WasmSignature,
    WasmType, WasmValue, WasmValueScalarExt,
};
pub mod conversion;
pub mod float;
pub mod i31;
pub mod integer;
pub(crate) mod numeric;
pub mod reference;
pub mod trap;
pub use trap::WasmTrap;
use control::{Control, Reachability};
use conversion::ScalarConversionOperator;
use float::{F32BinaryOperator, F32UnaryOperator, F64BinaryOperator, F64UnaryOperator};
use integer::{I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator};

const ZERO_LOCAL_CONSTANT: u32 = 0;
const VOID_RESULT_CONSTANT: u32 = 1;
/// The shared call ABI carries one Value; additional results need an owned bundle.
pub(crate) const SCALAR_RETURN_ARITY: usize = 1;

#[derive(Clone, Debug)]
pub struct WasmImportName {
    pub index: u32,
    pub module: String,
    pub name: String,
}

pub(crate) fn import_limits_match(
    current: u64,
    maximum: Option<u64>,
    expected_initial: u64,
    expected_maximum: Option<u64>,
) -> bool {
    current >= expected_initial
        && expected_maximum.is_none_or(|expected| maximum.is_some_and(|actual| actual <= expected))
}

/// Static reference expressions shared by table and element initializers.
#[derive(Clone)]
pub enum WasmReferenceInitializer {
    Null,
    Function(u32),
    Expression(WasmConstantExpression),
}

#[quench_macros::test_visible]
/// Shared immutable module facts; a module need not contain any function.
#[derive(Clone)]
pub struct WasmModule {
    pub(crate) program: Rc<ResidualProgram>,
    pub(crate) signatures: Rc<WasmSignatures>,
    pub(crate) globals: Rc<[WasmGlobal]>,
    pub(crate) memories: Rc<[WasmMemory]>,
    pub(crate) data: Rc<[WasmData]>,
    pub(crate) tables: Rc<[WasmTable]>,
    pub(crate) elements: Rc<[WasmElement]>,
    pub(crate) tags: Rc<[WasmTag]>,
    pub(crate) start: Option<u32>,
    pub(crate) exports: Rc<[WasmExport]>,
    /// The custom sections in module order, for `WebAssembly.Module.customSections`.
    pub(crate) custom_sections: Rc<[WasmCustomSection]>,
}

/// One custom section: its name and contents.
#[derive(Clone, Debug)]
pub struct WasmCustomSection {
    pub name: Rc<str>,
    pub bytes: Rc<[u8]>,
}

/// A typed entry derived from a module's canonical signature table.
#[derive(Clone)]
pub struct WasmFunction {
    pub(crate) module: WasmModule,
    pub(crate) entry: u32,
}

/// Validated global facts; runtime identity belongs to the instance's global cell.
#[derive(Clone, Debug)]
pub struct WasmGlobal {
    pub initial: WasmGlobalInitializer,
    pub mutable: bool,
}

/// Static values, imported bindings and ordered immutable dependencies.
#[derive(Clone, Debug)]
pub enum WasmGlobalInitializer {
    Expression(WasmConstantExpression),
    Import { ty: WasmType, name: WasmImportName },
}

impl From<WasmValue> for WasmGlobalInitializer {
    fn from(value: WasmValue) -> Self {
        Self::Expression(value.into())
    }
}

impl WasmGlobal {
    pub fn import(&self) -> Option<&WasmImportName> {
        match &self.initial {
            WasmGlobalInitializer::Import { name, .. } => Some(name),
            _ => None,
        }
    }

    /// The declaration contract owns the expected type, independently of runtime values.
    pub fn value_type(globals: &[Self], index: usize) -> Option<WasmType> {
        Some(match &globals.get(index)?.initial {
            WasmGlobalInitializer::Expression(expression) => expression.value_type(),
            WasmGlobalInitializer::Import { ty, .. } => *ty,
        })
    }
}

/// Derive host lookup names from the authoritative resource declarations.
/// Each import with the kind of external value it requires.
fn import_names<'a>(
    functions: &'a [scalar::CallableImport],
    globals: &'a [WasmGlobal],
    memories: &'a [WasmMemory],
    tables: &'a [WasmTable],
    tags: &'a [WasmTag],
) -> impl Iterator<Item = (&'a WasmImportName, wasmparser::ExternalKind)> {
    use wasmparser::ExternalKind;
    functions
        .iter()
        .map(|import| (&import.name, ExternalKind::Func))
        .chain(
            globals
                .iter()
                .filter_map(WasmGlobal::import)
                .map(|name| (name, ExternalKind::Global)),
        )
        .chain(
            memories
                .iter()
                .filter_map(|memory| memory.import.as_ref())
                .map(|name| (name, ExternalKind::Memory)),
        )
        .chain(tables.iter().filter_map(|table| match &table.initializer {
            WasmTableInitializer::Import(name) => Some((name, ExternalKind::Table)),
            WasmTableInitializer::Reference(_) => None,
        }))
        .chain(
            tags.iter()
                .filter_map(|tag| tag.import.as_ref())
                .map(|name| (name, ExternalKind::Tag)),
        )
}

/// A module export: its name and what it names in its kind's index space.
#[derive(Clone, Debug)]
pub struct WasmExport {
    pub name: String,
    pub kind: wasmparser::ExternalKind,
    pub index: u32,
}

/// Rooted instance bindings owned by one Runtime. Release with `release_wasm`.
pub struct WasmInstance {
    pub(crate) module: WasmModule,
    pub(crate) environment: crate::RootId,
}

/// Compatibility name for the i32-only lowering and execution boundaries.
pub type WasmI32Function = WasmFunction;

impl WasmModule {
    /// Source type-index facts, shared by every entry and instance of this module.
    pub fn types(&self) -> &WasmTypes {
        &self.signatures.declarations
    }

    /// Attach the validated start entry to immutable module facts.
    /// Instantiation executes it after active segments, before publishing the instance.
    pub fn with_start(mut self, index: u32) -> Result<Self, Diagnostic> {
        let signature = self.signatures.get(index as usize).ok_or_else(|| {
            Diagnostic::unsupported("wasm-start", "Wasm start function out of bounds")
        })?;
        if !signature.params.is_empty() || !signature.results.is_empty() {
            return Err(Diagnostic::unsupported(
                "wasm-start",
                "Wasm start function must have type [] -> []",
            ));
        }
        self.start = Some(index);
        Ok(self)
    }

    /// Imports in table-index order, derived from the table declarations.
    pub fn table_imports(&self) -> impl Iterator<Item = &WasmImportName> {
        self.tables
            .iter()
            .filter_map(|table| match &table.initializer {
                WasmTableInitializer::Import(name) => Some(name),
                WasmTableInitializer::Reference(_) => None,
            })
    }

    /// Import order is derived from declaration indices, independent of resource kind.
    pub fn imports(&self) -> Vec<&WasmImportName> {
        self.import_kinds()
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    /// Imports in declaration order, each with the kind of external value it requires.
    pub fn import_kinds(&self) -> Vec<(&WasmImportName, wasmparser::ExternalKind)> {
        let mut imports: Vec<_> = import_names(
            self.signatures.imports(),
            &self.globals,
            &self.memories,
            &self.tables,
            &self.tags,
        )
        .collect();
        imports.sort_by_key(|(name, _)| name.index);
        imports
    }

    /// Attach the export section's entries in declaration order.
    pub fn with_exports(mut self, exports: Vec<WasmExport>) -> Self {
        self.exports = exports.into();
        self
    }

    /// Attach the module's custom sections in module order.
    pub fn with_custom_sections(mut self, sections: Vec<WasmCustomSection>) -> Self {
        self.custom_sections = sections.into();
        self
    }

    /// The contents of the custom sections named `name`, in module order.
    pub fn custom_sections_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a [u8]> {
        self.custom_sections
            .iter()
            .filter(move |section| &*section.name == name)
            .map(|section| &*section.bytes)
    }

    pub fn exports(&self) -> &[WasmExport] {
        &self.exports
    }

    pub fn residual(&self) -> &ResidualProgram {
        &self.program
    }

    pub fn function(&self, index: u32) -> Option<WasmFunction> {
        self.signatures.get(index as usize)?;
        Some(WasmFunction {
            module: self.clone(),
            entry: index,
        })
    }
}

impl WasmFunction {
    pub fn residual(&self) -> &ResidualProgram {
        self.module.residual()
    }
    pub fn signature(&self) -> &WasmSignature {
        self.module
            .signatures
            .get(self.entry as usize)
            .expect("checked Wasm entry")
    }

    /// Derive another typed entry without copying the module residual.
    pub fn select_function(&self, index: u32) -> Option<Self> {
        self.module.function(index)
    }
}

impl Engine {
    /// Lower a decoded i32 function after the frontend validates its module.
    /// `locals` counts non-parameter i32 locals. No binary parser or second
    /// instruction representation is introduced at this boundary.
    pub fn lower_wasm_i32_function<'a>(
        name: &str,
        params: u16,
        locals: u16,
        has_result: bool,
        operators: impl IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    ) -> Result<WasmI32Function, Diagnostic> {
        Self::lower_wasm_i32_module(name, 0, [(params, locals, has_result, operators)])
    }

    /// Lower validated i32 function bodies into one shared residual program.
    /// Function indices and signatures retain the frontend's module order.
    pub fn lower_wasm_i32_module<'a, I>(
        name: &str,
        entry: u32,
        bodies: impl IntoIterator<Item = (u16, u16, bool, I)>,
    ) -> Result<WasmI32Function, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        Self::lower_wasm_module(
            name,
            entry,
            bodies
                .into_iter()
                .map(|(params, locals, has_result, operators)| WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(WasmSignature {
                        params: vec![WasmType::I32; usize::from(params)],
                        results: has_result.then_some(WasmType::I32).into_iter().collect(),
                    }),
                    locals: vec![wasmparser::ValType::I32; usize::from(locals)],
                    operators,
                }),
        )
    }

    /// Lower the frontend's validated scalar signatures and operator streams.
    pub fn lower_wasm_module<'a, I>(
        name: &str,
        entry: u32,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
    ) -> Result<WasmFunction, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        Self::lower_wasm_module_with_types(name, entry, bodies, &WasmTypes::default())
    }

    /// Consume the frontend's canonical module type-index domain for block signatures.
    /// Function signatures remain indexed by function, independently of type indices.
    pub fn lower_wasm_module_with_types<'a, I>(
        name: &str,
        entry: u32,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        types: &WasmTypes,
    ) -> Result<WasmFunction, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        Self::lower_wasm_module_with_globals(name, entry, bodies, types, &[])
    }

    /// Lower validated module globals to the shared lexical binding operations.
    pub fn lower_wasm_module_with_globals<'a, I>(
        name: &str,
        entry: u32,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        types: &WasmTypes,
        globals: &[WasmGlobal],
    ) -> Result<WasmFunction, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        Self::lower_wasm_module_definition(name, bodies, types, globals)?
            .function(entry)
            .ok_or_else(|| Diagnostic::unsupported(name, "Wasm entry function out of bounds"))
    }

    /// Lower a validated module independently of selecting an executable entry.
    pub fn lower_wasm_module_definition<'a, I>(
        name: &str,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        types: &WasmTypes,
        globals: &[WasmGlobal],
    ) -> Result<WasmModule, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        Self::lower_wasm_module_with_state(name, bodies, types, globals, &[], &[])
    }

    /// Lower module state into bindings traced by the shared heap.
    pub fn lower_wasm_module_with_state<'a, I>(
        name: &str,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        types: &WasmTypes,
        globals: &[WasmGlobal],
        memories: &[WasmMemory],
        data: &[WasmData],
    ) -> Result<WasmModule, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        Self::lower_wasm_module_with_tables(name, bodies, types, globals, memories, data, &[])
    }

    /// Lower validated tables into the same traced instance bindings.
    pub fn lower_wasm_module_with_tables<'a, I>(
        name: &str,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        types: &WasmTypes,
        globals: &[WasmGlobal],
        memories: &[WasmMemory],
        data: &[WasmData],
        tables: &[WasmTable],
    ) -> Result<WasmModule, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        Self::lower_wasm_module_with_elements(
            name,
            bodies,
            types,
            globals,
            memories,
            data,
            tables,
            &[],
        )
    }

    /// Element declarations and segments retain module order, independently of tables.
    pub fn lower_wasm_module_with_elements<'a, I>(
        name: &str,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        types: &WasmTypes,
        globals: &[WasmGlobal],
        memories: &[WasmMemory],
        data: &[WasmData],
        tables: &[WasmTable],
        elements: &[WasmElement],
    ) -> Result<WasmModule, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        Self::lower_wasm_module_with_imports(
            name,
            bodies,
            types,
            globals,
            memories,
            data,
            tables,
            elements,
            &[],
        )
    }

    /// Imported functions retain their source identity; only definitions produce bodies.
    pub fn lower_wasm_module_with_imports<'a, I>(
        name: &str,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        types: &WasmTypes,
        globals: &[WasmGlobal],
        memories: &[WasmMemory],
        data: &[WasmData],
        tables: &[WasmTable],
        elements: &[WasmElement],
        imports: &[WasmFunctionImport],
    ) -> Result<WasmModule, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        Self::lower_wasm_module_with_tags(
            name,
            bodies,
            types,
            globals,
            memories,
            data,
            tables,
            elements,
            imports,
            &[],
        )
    }

    /// Tags share the module's declaration graph and rooted instance bindings.
    pub fn lower_wasm_module_with_tags<'a, I>(
        name: &str,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        types: &WasmTypes,
        globals: &[WasmGlobal],
        memories: &[WasmMemory],
        data: &[WasmData],
        tables: &[WasmTable],
        elements: &[WasmElement],
        imports: &[WasmFunctionImport],
        tags: &[WasmTag],
    ) -> Result<WasmModule, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        Self::lower_wasm_module_for(
            crate::ExecutionMode::Interpreted,
            name,
            bodies,
            types,
            globals,
            memories,
            data,
            tables,
            elements,
            imports,
            tags,
        )
    }

    /// Lower a module for the execution mode that will run it: the residual
    /// shape (which calls are inlined) follows the mode's cost of a call;
    /// its meaning does not.
    #[allow(clippy::too_many_arguments)]
    pub fn lower_wasm_module_for<'a, I>(
        mode: crate::ExecutionMode,
        name: &str,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        types: &WasmTypes,
        globals: &[WasmGlobal],
        memories: &[WasmMemory],
        data: &[WasmData],
        tables: &[WasmTable],
        elements: &[WasmElement],
        imports: &[WasmFunctionImport],
        tags: &[WasmTag],
    ) -> Result<WasmModule, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>> + Clone,
    {
        let (function_types, sources): (Vec<_>, Vec<_>) = bodies
            .into_iter()
            .map(|body| (body.ty, inline::Source { locals: body.locals, operators: body.operators }))
            .unzip();
        Self::lower_wasm_sources(
            mode, name, function_types, &sources, None, types, globals, memories, data, tables,
            elements, imports, tags,
        )
    }

    /// A module whose functions lower on their first calls, for the interpreter: `code`
    /// keeps the bytes of its code section. Lowering runs once over every function at
    /// compile, which settles the module's constants and signatures and reports what does
    /// not lower; only a function that is called keeps its record.
    #[allow(clippy::too_many_arguments)]
    pub fn lower_wasm_module_lazily(
        name: &str,
        code: WasmLazyCode,
        types: &WasmTypes,
        globals: &[WasmGlobal],
        memories: &[WasmMemory],
        data: &[WasmData],
        tables: &[WasmTable],
        elements: &[WasmElement],
        imports: &[WasmFunctionImport],
        tags: &[WasmTag],
    ) -> Result<WasmModule, Diagnostic> {
        let code = Rc::new(code);
        let function_types = code
            .bodies
            .iter()
            .map(|body| WasmCallableType::Declared(body.ty))
            .collect();
        let sources: &WasmLazyCode = &code;
        Self::lower_wasm_sources(
            crate::ExecutionMode::Interpreted, name, function_types, &sources, Some(code.clone()),
            types, globals, memories, data, tables, elements, imports, tags,
        )
    }

    /// `lower_wasm_module_for` over a table of sources; `lazy`, when given, holds the
    /// bytes `sources` reads and the module lowers each function on its first call.
    #[allow(clippy::too_many_arguments)]
    fn lower_wasm_sources<'a, I>(
        mode: crate::ExecutionMode,
        name: &str,
        function_types: Vec<WasmCallableType>,
        sources: &dyn inline::SourceTable<'a, I>,
        lazy: Option<Rc<WasmLazyCode>>,
        types: &WasmTypes,
        globals: &[WasmGlobal],
        memories: &[WasmMemory],
        data: &[WasmData],
        tables: &[WasmTable],
        elements: &[WasmElement],
        imports: &[WasmFunctionImport],
        tags: &[WasmTag],
    ) -> Result<WasmModule, Diagnostic>
    where
        I: inline::Operators<'a>,
    {
        if tables
            .iter()
            .any(|table| !table::supported_table(&table.ty, types))
        {
            return Err(Diagnostic::unsupported(
                name,
                "unsupported Wasm table declaration",
            ));
        }
        if memories
            .iter()
            .any(|memory| !memory::supported_memory(&memory.ty))
        {
            return Err(Diagnostic::unsupported(name, "invalid Wasm memory limits"));
        }
        if globals
            .len()
            .checked_add(memories.len())
            .and_then(|len| len.checked_add(data.len()))
            .and_then(|len| len.checked_add(tables.len()))
            .and_then(|len| len.checked_add(elements.len()))
            .and_then(|len| len.checked_add(imports.len()))
            .and_then(|len| len.checked_add(tags.len()))
            .is_none_or(|len| len > usize::from(u16::MAX) + 1)
        {
            return Err(Diagnostic::unsupported(
                name,
                "too many Wasm instance bindings",
            ));
        }
        let mut signature_pool =
            WasmSignatures::with_imports(name, imports, function_types, types)?;
        for element in elements {
            if u32::try_from(element.items.len()).is_err() {
                return Err(Diagnostic::unsupported(name, "too many Wasm element items"));
            }
            if types
                .callable_value_type(wasmparser::ValType::Ref(element.element_type))
                .is_none()
            {
                return Err(Diagnostic::unsupported(
                    name,
                    "unsupported Wasm element reference type",
                ));
            }
            if let WasmElementMode::Active { table, .. } = &element.mode {
                tables.get(*table as usize).ok_or_else(|| {
                    Diagnostic::unsupported(name, "Wasm element table out of bounds")
                })?;
            }
        }
        for segment in data {
            if u32::try_from(segment.bytes.len()).is_err() {
                return Err(Diagnostic::unsupported(
                    name,
                    "Wasm data segment exceeds index domain",
                ));
            }
            if let WasmDataMode::Active { memory, .. } = &segment.mode {
                memories.get(*memory as usize).ok_or_else(|| {
                    Diagnostic::unsupported(name, "Wasm data memory index out of bounds")
                })?;
            }
        }
        let mut import_indices: Vec<_> =
            import_names(signature_pool.imports(), globals, memories, tables, tags)
                .map(|(name, _)| name.index)
                .collect();
        import_indices.sort_unstable();
        if import_indices
            .iter()
            .enumerate()
            .any(|(index, stored)| index as u64 != u64::from(*stored))
        {
            return Err(Diagnostic::unsupported(
                name,
                "invalid Wasm import index domain",
            ));
        }
        let mut constants = ConstantPool::with([Constant::Number(0.0), Constant::Undefined]);
        let lazy_lowering = lazy.is_some();
        let mut functions = Vec::with_capacity(if lazy_lowering { 0 } else { sources.count() });
        let mut slots = Vec::with_capacity(if lazy_lowering { sources.count() } else { 0 });
        let plan = inline::LeafPlan::new(
            name,
            sources,
            inline::Bindings {
                memories,
                globals: globals.len(),
            },
            &signature_pool,
            inline::leaf_budget(mode, || {
                (0..sources.count())
                    .map(|index| sources.operators(index).map_or(0, |operators| operators.into_iter().count()))
                    .sum()
            }),
        )?;
        let bindings = ModuleBindings { name, globals, memories, data, tables, elements, tags };
        for position in 0..sources.count() {
            let function = bindings.lower_defined(
                position,
                &plan,
                SignaturePool::Building(&mut signature_pool),
                &mut constants,
                &[],
            )?;
            if !lazy_lowering {
                functions.push(function);
            } else if position == 0 {
                // A lazily lowered module keeps its first record: an environment's root
                // function is read without a call.
                slots.push(crate::bytecode::FunctionSlot::compiled(function));
            } else {
                // Kept by its first call; this lowering settled what it interns.
                slots.push(crate::bytecode::FunctionSlot::released(
                    crate::bytecode::FunctionHeader::of(&function),
                ));
            }
        }
        let facts = plan.facts();
        let signatures = Rc::new(signature_pool);
        let globals: Rc<[WasmGlobal]> = globals.into();
        let memories: Rc<[WasmMemory]> = memories.into();
        let data: Rc<[WasmData]> = data.into();
        let tables: Rc<[WasmTable]> = tables.into();
        let elements: Rc<[WasmElement]> = elements.into();
        let tags: Rc<[WasmTag]> = tags.into();
        let (program_constants, kept_constants) = if lazy_lowering {
            let (settled, pool) = constants.settle();
            (settled, Some(pool))
        } else {
            (constants.into_constants(), None)
        };
        if !lazy_lowering {
            slots = functions.into_iter().map(crate::bytecode::FunctionSlot::compiled).collect();
        }
        let wasm_lazy = lazy.zip(kept_constants).map(|(code, constants)| {
            Rc::new(WasmLazy {
                name: name.into(),
                code,
                facts,
                constants: std::cell::RefCell::new(constants),
                signatures: signatures.clone(),
                globals: globals.clone(),
                memories: memories.clone(),
                data: data.clone(),
                tables: tables.clone(),
                elements: elements.clone(),
                tags: tags.clone(),
            })
        });
        let program = ResidualProgram {
            specialized: false,
            kind: ProgramKind::Wasm,
            module_requests: vec![],
            module_imports: vec![],
            module_link_plan: None,
            declared_types: None,
            source_name: name.into(),
            source: None,
            record: crate::bytecode::ProgramRecord { atoms: AtomTable::default(), constants: program_constants.into() },
            functions: slots.into(),
            cache_sites: Default::default(),
            method_sites: vec![].into(),
            method_arguments: vec![].into(),
            field_sites: vec![].into(),
            object_sites: vec![].into(),
            regexp_literal_sites: vec![].into(),
            literal_templates: Vec::new().into(),
            superinstructions: vec![].into(), rewrite_ranges: Default::default(),
            replay: None, first_call_scopes: Default::default(),
            encoded_records: None,
            validated: Default::default(),
            undefined_constant: Default::default(),
            load_encoding: Default::default(),
            closure_reads: None,
            wasm_lazy,
        };
        program.validate().map_err(|e| Diagnostic::unsupported(name, e))?;
        program.validated.establish();
        Ok(WasmModule {
            program: Rc::new(program),
            signatures,
            globals,
            memories,
            data,
            tables,
            elements,
            tags,
            start: None,
            exports: Rc::default(),
            custom_sections: Rc::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowering_rejects_invalid_control_structure_and_stack_domains() {
        for operators in [
            vec![Operator::Else, Operator::End],
            vec![
                Operator::Br {
                    relative_depth: u32::MAX,
                },
                Operator::End,
            ],
            vec![
                Operator::Block {
                    blockty: wasmparser::BlockType::Empty,
                },
                Operator::End,
            ],
            vec![
                Operator::I32Const { value: 1 },
                Operator::Block {
                    blockty: wasmparser::BlockType::Empty,
                },
                Operator::Drop,
                Operator::End,
                Operator::End,
            ],
            vec![
                Operator::I32Const { value: 1 },
                Operator::If {
                    blockty: wasmparser::BlockType::Type(wasmparser::ValType::I32),
                },
                Operator::I32Const { value: 2 },
                Operator::End,
                Operator::End,
            ],
        ] {
            assert!(
                Engine::lower_wasm_i32_function(
                    "invalid control",
                    0,
                    0,
                    false,
                    operators.into_iter().map(Ok)
                )
                .is_err()
            );
        }
    }

    #[test]
    fn hidden_if_inputs_do_not_expand_the_guest_local_index_domain() {
        let types = WasmTypes::from_functions([wasmparser::FuncType::new(
            [wasmparser::ValType::I32],
            [wasmparser::ValType::I32],
        )]);
        let result = Engine::lower_wasm_module_with_types(
            "hidden input local",
            0,
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![],
                    results: vec![WasmType::I32],
                }),
                locals: vec![],
                operators: [
                    Operator::I32Const { value: 3 },
                    Operator::I32Const { value: 0 },
                    Operator::If {
                        blockty: wasmparser::BlockType::FuncType(0),
                    },
                    Operator::I32Const { value: 4 },
                    Operator::I32Add,
                    Operator::Else,
                    Operator::LocalGet { local_index: 0 },
                    Operator::I32Add,
                    Operator::End,
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            }],
            &types,
        );
        assert!(
            result.is_err(),
            "compiler scratch locals are not guest declarations"
        );
    }

    #[test]
    fn lowering_boundary_rejects_invalid_operand_and_local_domains() {
        for operators in [
            vec![Operator::I32Add, Operator::End],
            vec![Operator::Call { function_index: 1 }, Operator::End],
            vec![Operator::Call { function_index: 0 }, Operator::End],
            vec![Operator::LocalGet { local_index: 1 }, Operator::End],
            vec![Operator::LocalSet { local_index: 0 }, Operator::End],
            vec![Operator::I32Const { value: 1 }],
            vec![Operator::End],
            vec![
                Operator::I32Const { value: 1 },
                Operator::End,
                Operator::Nop,
            ],
        ] {
            assert!(
                Engine::lower_wasm_i32_function(
                    "invalid",
                    1,
                    0,
                    true,
                    operators.into_iter().map(Ok)
                )
                .is_err()
            );
        }
        assert!(
            Engine::lower_wasm_i32_function("locals", u16::MAX, 1, false, [Ok(Operator::End)])
                .is_err()
        );
    }

    #[test]
    fn lowering_rejects_stack_depth_outside_result_register_layout() {
        let operators = std::iter::repeat_with(|| Ok(Operator::I32Const { value: 1 }))
            .take(usize::from(crate::bytecode::REGISTER_MASK) + 2);
        assert!(Engine::lower_wasm_i32_function("stack", 0, 0, true, operators).is_err());
    }

    #[test]
    fn residual_validation_rejects_unknown_wasm_numeric_selectors() {
        for (operator, opcode, ty, result_type) in [
            (
                Operator::I32Eqz,
                Op::WasmI32Unary,
                WasmType::I32,
                WasmType::I32,
            ),
            (
                Operator::I64Add,
                Op::WasmI64Binary,
                WasmType::I64,
                WasmType::I64,
            ),
            (
                Operator::I64Clz,
                Op::WasmI64Unary,
                WasmType::I64,
                WasmType::I64,
            ),
            (
                Operator::I32WrapI64,
                Op::WasmScalarConvert,
                WasmType::I64,
                WasmType::I32,
            ),
            (
                Operator::F32Add,
                Op::WasmF32Binary,
                WasmType::F32,
                WasmType::F32,
            ),
            (
                Operator::F32Neg,
                Op::WasmF32Unary,
                WasmType::F32,
                WasmType::F32,
            ),
            (
                Operator::F64Add,
                Op::WasmF64Binary,
                WasmType::F64,
                WasmType::F64,
            ),
            (
                Operator::F64Neg,
                Op::WasmF64Unary,
                WasmType::F64,
                WasmType::F64,
            ),
        ] {
            let mut operators = vec![Operator::LocalGet { local_index: 0 }];
            if opcode
                .field_layout(crate::bytecode::InstructionField::C)
                .is_register_field()
            {
                operators.push(Operator::LocalGet { local_index: 0 });
            }
            operators.extend([operator, Operator::End]);
            let mut function = Engine::lower_wasm_module(
                "selector",
                0,
                [WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(WasmSignature {
                        params: vec![ty],
                        results: vec![result_type],
                    }),
                    locals: vec![],
                    operators: operators.into_iter().map(Ok),
                }],
            )
            .unwrap();
            // The mutated program is a clone, which starts unproven (`ValidationProof`).
            let held = Rc::clone(&function.module.program);
            let residual = &mut Rc::make_mut(&mut function.module.program).functions[0];
            let pc = residual.code.iter().position(|i| i.op() == opcode).unwrap();
            let index = residual.wide.len();
            let instruction = residual.code[pc];
            residual.wide.push(WideInstruction::new(
                opcode,
                instruction.a(),
                instruction.b(),
                instruction.c(),
                u32::MAX,
            ));
            residual.code[pc] = Instr::wide(index).unwrap();
            assert!(function.module.program.validate().is_err() && !Rc::ptr_eq(&held, &function.module.program));
            let error = crate::Runtime::new(crate::SystemHost).execute_wasm(&function, &[ty.default_value().unwrap()]).unwrap_err();
            assert_eq!(error.wasm_trap(), None);
        }
    }

    #[test]
    fn scalar_conversion_rows_own_input_output_types_and_selector_domain() {
        assert_eq!(ScalarConversionOperator::from_tag(u32::MAX), None);
        for operator in ScalarConversionOperator::ALL {
            let input = operator.source_type().default_value().unwrap();
            let output = operator.apply(input).unwrap();
            assert_eq!(input.ty(), operator.source_type());
            assert_eq!(output.ty(), operator.result_type());
        }
    }

    #[test]
    fn serialized_float_rows_preserve_rounding_and_nan_classes() {
        let mut function = Engine::lower_wasm_module(
            "float round trip",
            0,
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![WasmType::F64; 2],
                    results: vec![WasmType::F64],
                }),
                locals: vec![],
                operators: [
                    Operator::LocalGet { local_index: 0 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::F64Max,
                    Operator::F64Nearest,
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            }],
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-shared-float-{}.qbc", std::process::id()));
        function.module.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.module.program = Rc::new(decoded.unwrap());
        let mut runtime = crate::Runtime::new(crate::SystemHost);
        assert_eq!(
            runtime
                .execute_wasm(
                    &function,
                    &[
                        WasmValue::F64(1.0f64.to_bits()),
                        WasmValue::F64(2.5f64.to_bits())
                    ]
                )
                .unwrap(),
            Some(WasmValue::F64(2.0f64.to_bits()))
        );
        assert!(
            runtime
                .execute_wasm(
                    &function,
                    &[
                        WasmValue::F64(0x7ff0_0000_0000_0001),
                        WasmValue::F64(1.0f64.to_bits())
                    ]
                )
                .unwrap()
                .unwrap()
                .is_canonical_nan()
        );
    }

    #[test]
    fn serialized_i64_numeric_rows_execute_after_decoding() {
        let mut function = Engine::lower_wasm_module(
            "i64 round trip",
            0,
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![WasmType::I64; 2],
                    results: vec![WasmType::I64],
                }),
                locals: vec![],
                operators: [
                    Operator::LocalGet { local_index: 0 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::I64DivU,
                    Operator::I64Clz,
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            }],
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-shared-i64-{}.qbc", std::process::id()));
        function.module.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.module.program = Rc::new(decoded.unwrap());
        let mut runtime = crate::Runtime::new(crate::SystemHost);
        assert_eq!(
            runtime
                .execute_wasm(&function, &[WasmValue::I64(-1), WasmValue::I64(2)])
                .unwrap(),
            Some(WasmValue::I64(1))
        );
        assert_eq!(
            runtime
                .execute_wasm(&function, &[WasmValue::I64(1), WasmValue::I64(0)])
                .unwrap_err()
                .wasm_trap(),
            Some(WasmTrap::IntegerDivideByZero)
        );
    }

    #[test]
    fn serialized_wasm_bits64_preserve_tag_collision_payloads() {
        let bits = 0x7ffc_1234_5678_9abc_u64;
        let mut function = Engine::lower_wasm_module(
            "scalar round trip",
            0,
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![],
                    results: vec![WasmType::I64],
                }),
                locals: vec![],
                operators: [Operator::I64Const { value: bits as i64 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            }],
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-shared-scalars-{}.qbc", std::process::id()));
        function.module.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.module.program = Rc::new(decoded.unwrap());
        assert_eq!(
            crate::Runtime::new(crate::SystemHost)
                .execute_wasm(&function, &[])
                .unwrap(),
            Some(WasmValue::I64(bits as i64))
        );
    }

    #[test]
    fn serialized_wasm_numeric_rows_execute_after_decoding() {
        let operators = [
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32DivU,
            Operator::I32Clz,
            Operator::End,
        ];
        let mut function = Engine::lower_wasm_i32_function(
            "round-trip",
            2,
            0,
            true,
            operators.into_iter().map(Ok),
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-shared-i32-{}.qbc", std::process::id()));
        function.module.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.module.program = Rc::new(decoded.unwrap());
        let mut runtime = crate::Runtime::new(crate::SystemHost);
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[-1, 2]).unwrap(),
            Some(1)
        );
        assert_eq!(
            runtime
                .execute_wasm_i32(&function, &[1, 0])
                .unwrap_err()
                .wasm_trap(),
            Some(WasmTrap::IntegerDivideByZero)
        );
    }
}


/// The code section of a module whose functions lower on their first calls, kept so each
/// body can be read again: one copy of its bytes and each body's range in them.
pub struct WasmLazyCode {
    bytes: Box<[u8]>,
    /// The module offset of `bytes[0]`.
    base: usize,
    bodies: Box<[WasmLazyBody]>,
}

/// A defined function's declared type and its body's module range.
#[derive(Clone, Copy)]
pub struct WasmLazyBody {
    pub ty: u32,
    pub start: u32,
    pub end: u32,
}

impl WasmLazyCode {
    /// `bytes` are the module's from offset `base` on and hold every body in `bodies`.
    pub fn new(bytes: Box<[u8]>, base: usize, bodies: Vec<WasmLazyBody>) -> Self {
        Self { bytes, base, bodies: bodies.into() }
    }

    fn body(&self, index: usize) -> Result<wasmparser::FunctionBody<'_>, Diagnostic> {
        let body = self.bodies.get(index).ok_or_else(|| Diagnostic::unsupported("wasm", "unknown Wasm function body"))?;
        let start = (body.start as usize).checked_sub(self.base);
        let bytes = start.and_then(|start| self.bytes.get(start..body.end as usize - self.base));
        let bytes = bytes.ok_or_else(|| Diagnostic::unsupported("wasm", "Wasm function body outside its code"))?;
        Ok(wasmparser::FunctionBody::new(wasmparser::BinaryReader::new(bytes, body.start as usize)))
    }
}

impl<'b> inline::SourceTable<'b, wasmparser::OperatorsReader<'b>> for &'b WasmLazyCode {
    fn count(&self) -> usize {
        self.bodies.len()
    }

    fn locals(&self, index: usize) -> Result<Vec<wasmparser::ValType>, Diagnostic> {
        let error = |message: &str| Diagnostic::unsupported("wasm", message);
        let mut locals = Vec::new();
        let reader = self.body(index)?.get_locals_reader().map_err(|error| Diagnostic::unsupported("wasm", &error.to_string()))?;
        for local in reader {
            let (count, ty) = local.map_err(|error| Diagnostic::unsupported("wasm", &error.to_string()))?;
            let length = usize::try_from(count)
                .ok()
                .and_then(|count| locals.len().checked_add(count))
                .filter(|length| *length <= usize::from(u16::MAX))
                .ok_or_else(|| error("too many locals"))?;
            locals.resize(length, ty);
        }
        Ok(locals)
    }

    fn operators(&self, index: usize) -> Result<wasmparser::OperatorsReader<'b>, Diagnostic> {
        let this: &'b WasmLazyCode = self;
        this.body(index)?.get_operators_reader().map_err(|error| Diagnostic::unsupported("wasm", &error.to_string()))
    }
}

/// What a module whose functions lower on their first calls keeps to lower them: its code,
/// its inlining plan's decisions, and the constants and signatures its compile settled.
pub(crate) struct WasmLazy {
    name: Rc<str>,
    code: Rc<WasmLazyCode>,
    facts: inline::PlanFacts,
    constants: std::cell::RefCell<ConstantPool>,
    signatures: Rc<WasmSignatures>,
    globals: Rc<[WasmGlobal]>,
    memories: Rc<[WasmMemory]>,
    data: Rc<[WasmData]>,
    tables: Rc<[WasmTable]>,
    elements: Rc<[WasmElement]>,
    tags: Rc<[WasmTag]>,
}

impl std::fmt::Debug for WasmLazy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmLazy").field("name", &self.name).field("functions", &self.code.bodies.len()).finish()
    }
}

impl WasmLazy {
    /// The record of function `index`, lowered as the module's compile lowered it, against
    /// a program that holds `program_constants`.
    pub(crate) fn lower(&self, index: usize, program_constants: &[Constant]) -> Function {
        let sources: &WasmLazyCode = &self.code;
        let plan = inline::LeafPlan::resume(
            &self.name,
            &sources,
            inline::Bindings { memories: &self.memories, globals: self.globals.len() },
            &self.facts,
        );
        let bindings = ModuleBindings {
            name: &self.name,
            globals: &self.globals,
            memories: &self.memories,
            data: &self.data,
            tables: &self.tables,
            elements: &self.elements,
            tags: &self.tags,
        };
        let mut constants = self.constants.borrow_mut();
        let function = bindings
            .lower_defined(index, &plan, SignaturePool::Finished(&self.signatures), &mut constants, program_constants)
            .unwrap_or_else(|error| panic!("Wasm function {index} of {} lowered at compile but not again: {error:?}", self.name));
        assert_eq!(
            constants.len(),
            program_constants.len(),
            "Wasm function {index} of {} interned a constant its compile did not",
            self.name
        );
        function
    }
}

/// The module state every defined function's lowering reads.
#[derive(Clone, Copy)]
struct ModuleBindings<'a> {
    name: &'a str,
    globals: &'a [WasmGlobal],
    memories: &'a [WasmMemory],
    data: &'a [WasmData],
    tables: &'a [WasmTable],
    elements: &'a [WasmElement],
    tags: &'a [WasmTag],
}

impl ModuleBindings<'_> {
    /// The layout of an instance's environment (`WasmLayout`): its one constructor, which
    /// `WasmModule::layout` and each lowering read.
    fn layout(&self) -> WasmLayout {
        WasmLayout {
            globals: self.globals.len(),
            memories: self.memories.len(),
            data: self.data.len(),
            tables: self.tables.len(),
            elements: self.elements.len(),
            tags: self.tags.len(),
        }
    }

    /// The record of defined function `position`, streamed, inlined, from its source as
    /// `plan` inlines it, its constants interned into `constants`.
    fn lower_defined<'s, I: inline::Operators<'s>>(
        &self,
        position: usize,
        plan: &inline::LeafPlan<'_, 's, I>,
        signatures: SignaturePool<'_>,
        constants: &mut ConstantPool,
        settled: &[Constant],
    ) -> Result<Function, Diagnostic> {
        let layout = self.layout();
        let Self { name, globals, memories, data, tables, elements, tags } = *self;
        // The body is streamed, inlined, from its source.
        let form = plan.lowered(position)?;
        let index = u32::try_from(position)
            .map_err(|_| Diagnostic::unsupported(name, "too many Wasm functions"))?;
        let signature = signatures.defined_signature(index).unwrap().clone();
        let params = u16::try_from(signature.params.len())
            .map_err(|_| Diagnostic::unsupported(name, "too many Wasm parameters"))?;
        let results = u16::try_from(signature.results.len())
            .map_err(|_| Diagnostic::unsupported(name, "too many Wasm function results"))?;
        let error = |message: &str| Diagnostic::unsupported(name, message);
        let local_count = usize::from(params)
            .checked_add(form.locals.len())
            .and_then(|count| u16::try_from(count).ok())
            .ok_or_else(|| error("too many Wasm locals"))?;
        // Instance bindings the body uses get registers loaded once per call.
        let mut reserved: Register = 0;
        let mut reserve = |used: bool| {
            let register = used.then_some(reserved);
            reserved += Register::from(used);
            register
        };
        let memory_registers: Vec<_> =
            form.memories.iter().copied().map(&mut reserve).collect();
        // The accessed globals, ascending, with their registers, rather than a slot per global
        // of the module: a module of many globals and functions paid globals x functions.
        let global_registers: Vec<_> = form.globals.iter().map(|&global| (global, reserve(true).expect("a reserved register"))).collect();
        let local_base = (usize::from(reserved) + usize::from(local_count)
            <= MAX_REGISTER_LOCALS)
            .then_some(reserved);
        if local_base.is_some() {
            reserved += local_count;
        }
        let mut lowering = Lowering {
            fusion_floor: 0,
            name,
            locals: local_count,
            temporary_base: if local_base.is_some() { 0 } else { local_count },
            temporary_locals: 0,
            memory_registers,
            global_registers,
            local_base,
            aliases: Vec::new(),
            producer: None,
            previous: None,
            code: Vec::new(),
            wide: Vec::new(),
            constants: std::mem::replace(constants, ConstantPool::with(std::iter::empty())),
            settled_constants: settled,
            signatures,
            globals,
            memories,
            data,
            tables,
            elements,
            tags,
            layout,
            handlers: vec![],
            depth: reserved,
            // The function base register always exists: it carries a void result.
            registers: reserved + 1,
            controls: vec![Control::function(reserved, results)],
            path: Reachability::Live,
        };
        // Defaultable locals get residual defaults; others remain uninitialized.
        // The frontend proves assignment before non-defaultable local reads.
        if let Some(base) = local_base {
            lowering.fill_register_locals(base, params, &form.locals, &form.assigned_first)?;
        } else {
            for (index, ty) in form.locals.iter().copied().enumerate() {
                if ty.is_defaultable() {
                    let slot = usize::from(params) + index;
                    lowering.load_default(0, ty)?;
                    lowering.emit_emission(wasm_emit::store_local(0, slot as u16))?;
                }
            }
        }
        let global_slots = lowering.global_registers.iter().map(|&(global, register)| (global as usize, register));
        let memory_slots = lowering.memory_registers.iter().enumerate()
            .filter_map(|(memory, register)| Some((lowering.layout.slot(WasmBinding::Memory(memory)), (*register)?)));
        let bindings: Vec<_> = global_slots.chain(memory_slots).collect();
        for (slot, register) in bindings {
            lowering.emit_emission(wasm_emit::instance_binding(register, slot as u32))?;
        }
        plan.lower(position, &form, &mut |operator| {
            if lowering.controls.is_empty() {
                return Err(error("operators after Wasm function end"));
            }
            lowering.operator(operator)
        })?;
        if !lowering.controls.is_empty() {
            return Err(error("missing Wasm function end"));
        }
        let lowering_constants = lowering.constants;
        let function = Function {
            parent: None,
            name: None,
            inferred_name: None,
            is_arrow: false,
            self_binding_slot: None,
            source_span: None,
            params,
            length: params,
            parameter_end_pc: 0,
            parameter_atoms: vec![].into(),
            rare: None,
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            constant_fields: Default::default(),
            super_home_atom: None,
            constructible: false,
            class_initializer: false,
            parameter_eval_arguments_error: false,
            unaliased_arguments: false, rest_alias: false,
        prototype_unobservable: false,
            arguments_slot: None,
            simple_parameters: true,
            strict: true,
            locals: lowering.temporary_base + lowering.temporary_locals,
            local_atoms: vec![].into(),
            environment_atoms: vec![].into(),
            selective_capture_slots: None,
            declared_functions: Default::default(),
            declared_captures: Default::default(),
            local_registers: Vec::new().into(),
            inherited_with_scope: false,
            lexical_atoms: vec![].into(),
            name_bindings: Default::default(),
            binding_sites: vec![].into(),
            source_positions: Default::default(),
            environment_clones: vec![].into(),
            code: lowering.code,
            wide: lowering.wide,
            registers: lowering.registers,
            dispatch: Default::default(),
            decoded: Default::default(),
            plain_locals: Default::default(), caches: Default::default(),
            lazy_flags: Default::default(),
            call_plan: Default::default(),
            handlers: lowering.handlers.into(),
            register_roots: Default::default(),
            undefined_register: None,
            parameter_registers: lowering.local_base,
            initial_register: crate::bytecode::InitialRegister::I32Zero,
        };
        // The lowering grew the code by doubling; the module keeps it for its life.
        let mut function = function;
        function.shrink_to_fit();
        // i64 values no boxed use reaches live in their registers as machine words: their ops
        // then run as the C op plus dispatch and decode (`compile::liveness::raw_webs`).
        let constant_neutral = |index: usize| {
            matches!(lowering_constants.get(settled, index), Some(Constant::WasmBits64(bits)) if crate::Value::from_bits64(*bits).is_some())
        };
        let returns_wide = matches!(signature.results.as_slice(), [WasmType::I64 | WasmType::F64]);
        // The raw switch solved the function's live sets; its root masks are built from them,
        // so the function is solved once.
        let solved = crate::compile::liveness::raw_webs::use_raw_forms(&mut function, &constant_neutral, returns_wide);
        let no_method_sites: &[crate::compile::liveness::MethodSite] = &[];
        function.register_roots = crate::compile::liveness::register_roots_from(&function, no_method_sites, &[], &[], solved);
        // Every Wasm local is a frame slot by construction: no environment, dead zone or name
        // binding can observe it (`ResidualProgram::validate`). The lowering records that set,
        // as `compile::lower_settled` records a script function's proof, so the first query
        // (`Function::plain_local`, the call plan's `reflects_live_parameters`) does not derive
        // the script rule, which finds no binding atoms for these slots and proves none plain.
        let plain = crate::bitset::BitSet::from_fn(usize::from(function.locals), |_| true);
        let words: crate::heap::ThinBoxSlice<u64> = plain.words().to_vec().into();
        function.plain_locals = std::cell::OnceCell::from(words);
        *constants = lowering_constants;
        Ok(function)
    }
}

/// The signature pool a lowering interns into: the module's while it is being lowered,
/// or the finished pool of a module whose functions lower on their first calls, which
/// already holds every type they intern.
enum SignaturePool<'a> {
    Building(&'a mut WasmSignatures),
    Finished(&'a WasmSignatures),
}

impl SignaturePool<'_> {
    fn intern(&mut self, name: &str, ty: WasmCallableType) -> Result<u32, Diagnostic> {
        match self {
            Self::Building(pool) => pool.intern(name, ty),
            Self::Finished(pool) => pool.interned_index(name, ty),
        }
    }
}

impl std::ops::Deref for SignaturePool<'_> {
    type Target = WasmSignatures;

    fn deref(&self) -> &WasmSignatures {
        match self {
            Self::Building(pool) => pool,
            Self::Finished(pool) => pool,
        }
    }
}

struct Lowering<'a> {
    /// Instructions from here on follow the last label, so a later
    /// instruction may fuse into them without a jump landing between.
    fusion_floor: usize,
    signatures: SignaturePool<'a>,
    globals: &'a [WasmGlobal],
    memories: &'a [WasmMemory],
    data: &'a [WasmData],
    tables: &'a [WasmTable],
    elements: &'a [WasmElement],
    tags: &'a [WasmTag],
    /// The layout of the instance environment the lowered code reads (`ModuleBindings::layout`).
    layout: WasmLayout,
    handlers: Vec<crate::bytecode::Handler>,
    name: &'a str,
    locals: u16,
    // Frame slots for exception payloads and saved block inputs start here:
    // after the Wasm locals, or at zero when those live in registers.
    temporary_base: u16,
    temporary_locals: u16,
    // Prologue-loaded binding register per memory used by direct accesses.
    memory_registers: Vec<Option<Register>>,
    // Prologue-loaded binding register per global the body reads or writes, by global index.
    global_registers: Vec<(u32, Register)>,
    // First register of the Wasm locals when they live in frame registers.
    local_base: Option<Register>,
    // Operand-stack positions whose value is still a local register or a
    // constant, by position; materialized only when a reader needs the slot.
    aliases: Vec<Option<Alias>>,
    // The instruction this operator emitted for the operand-stack top.
    producer: Option<(usize, Register)>,
    // `producer` as left by the previous operator.
    previous: Option<(usize, Register)>,
    code: Vec<Instr>,
    wide: Vec<WideInstruction>,
    constants: ConstantPool,
    // The program's constants a settled `constants` indexes (empty while compiling).
    settled_constants: &'a [Constant],
    depth: Register,
    registers: u16,
    controls: Vec<Control>,
    path: Reachability,
}

/// An operand-stack value not yet copied into its position's register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Alias {
    Local(Register),
    I32(i32),
    /// An i64 constant small enough to be an operator's 16-bit constant operand.
    I64(i16),
}

/// Locals live in frame registers while the whole register file stays small.
const MAX_REGISTER_LOCALS: usize = 4096;

/// How a scalar numeric operator lowers: i32 binary operators name their own
/// opcode; the remaining families carry their operator as a selector.
enum NumericLowering {
    I32Binary(I32BinaryOperator),
    Selector(SelectorFamily, u32),
}

fn numeric_operator(operator: &Operator<'_>) -> Option<NumericLowering> {
    I32BinaryOperator::from_wasm(operator)
        .map(NumericLowering::I32Binary)
        .or_else(|| {
            selector_numeric_operator(operator)
                .map(|(op, selector)| NumericLowering::Selector(op, selector))
        })
}

fn selector_numeric_operator(operator: &Operator<'_>) -> Option<(SelectorFamily, u32)> {
    I32UnaryOperator::from_wasm(operator)
        .map(|op| (SelectorFamily::I32Unary, op as u32))
        .or_else(|| I64BinaryOperator::from_wasm(operator).map(|op| (SelectorFamily::I64Binary, op as u32)))
        .or_else(|| I64UnaryOperator::from_wasm(operator).map(|op| (SelectorFamily::I64Unary, op as u32)))
        .or_else(|| {
            ScalarConversionOperator::from_wasm(operator)
                .map(|op| (SelectorFamily::ScalarConvert, op as u32))
        })
        .or_else(|| F32BinaryOperator::from_wasm(operator).map(|op| (SelectorFamily::F32Binary, op as u32)))
        .or_else(|| F32UnaryOperator::from_wasm(operator).map(|op| (SelectorFamily::F32Unary, op as u32)))
        .or_else(|| F64BinaryOperator::from_wasm(operator).map(|op| (SelectorFamily::F64Binary, op as u32)))
        .or_else(|| F64UnaryOperator::from_wasm(operator).map(|op| (SelectorFamily::F64Unary, op as u32)))
}

#[derive(Clone, Copy)]
enum WasmCallTarget {
    Known(u16),
    Reference(Register),
}

impl Lowering<'_> {
    /// The register the prologue loads `global`'s binding into, if the body accesses it.
    fn global_register(&self, global: u32) -> Option<Register> {
        let at = self.global_registers.binary_search_by_key(&global, |&(used, _)| used).ok()?;
        Some(self.global_registers[at].1)
    }

    fn emit(
        &mut self,
        op: Op,
        a: Register,
        b: Register,
        c: Register,
        imm: u32,
    ) -> Result<(), Diagnostic> {
        let instruction = match Instr::try_new(op, a, b, c, imm) {
            Some(instruction) => instruction,
            None => {
                let marker = Instr::wide(self.wide.len())
                    .ok_or_else(|| Diagnostic::unsupported(self.name, "Wasm residual too large"))?;
                self.wide.push(WideInstruction::new(op, a, b, c, imm));
                marker
            }
        };
        self.code.push(instruction);
        Ok(())
    }

    /// Emits `instruction` through the lowering's one instruction buffer (`Lowering::emit`).
    fn emit_emission(&mut self, instruction: Emission) -> Result<(), Diagnostic> {
        self.emit(
            instruction.op,
            instruction.a,
            instruction.b,
            instruction.c,
            instruction.imm,
        )
    }

    fn push(&mut self) -> Result<Register, Diagnostic> {
        let register = self.depth;
        if register > crate::bytecode::REGISTER_MASK {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm operand stack exceeds register layout",
            ));
        }
        self.depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| Diagnostic::unsupported(self.name, "Wasm operand stack too large"))?;
        self.registers = self.registers.max(self.depth);
        if let Some(alias) = self.aliases.get_mut(usize::from(register)) {
            *alias = None;
        }
        Ok(register)
    }

    /// Pop the operand register an operator reads: a local register while the
    /// position still aliases that local, otherwise the position's own register.
    fn pop(&mut self) -> Result<Register, Diagnostic> {
        let position = self.pop_position()?;
        match self.take_alias(position) {
            Some(Alias::Local(local)) => Ok(local),
            Some(Alias::I32(value)) => {
                self.load_i32(position, value)?;
                Ok(position)
            }
            Some(Alias::I64(value)) => {
                self.load_scalar(position, WasmValue::I64(i64::from(value)))?;
                Ok(position)
            }
            None => Ok(position),
        }
    }

    fn take_alias(&mut self, position: Register) -> Option<Alias> {
        self.aliases
            .get_mut(usize::from(position))
            .and_then(Option::take)
    }

    /// The constant on the operand-stack top, while it is still unmaterialized.
    fn top_i32_constant(&self) -> Option<i32> {
        match self.top_alias()? {
            Alias::I32(value) => Some(value),
            _ => None,
        }
    }

    /// The small i64 constant on the operand-stack top, while it is still unmaterialized.
    fn top_i64_constant(&self) -> Option<i16> {
        match self.top_alias()? {
            Alias::I64(value) => Some(value),
            _ => None,
        }
    }

    fn top_alias(&self) -> Option<Alias> {
        let top = usize::from(self.depth.checked_sub(1)?);
        match self.aliases.get(top) {
            Some(Some(alias)) if self.depth > self.control_base() => Some(*alias),
            _ => None,
        }
    }

    fn pop_position(&mut self) -> Result<Register, Diagnostic> {
        if self.depth <= self.control_base() {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm operand stack underflow",
            ));
        }
        self.depth = self
            .depth
            .checked_sub(1)
            .ok_or_else(|| Diagnostic::unsupported(self.name, "Wasm operand stack underflow"))?;
        Ok(self.depth)
    }

    /// Copy every aliased operand-stack position into its own register, so
    /// operators that address positions directly observe ordinary values.
    fn materialize_aliases(&mut self) -> Result<(), Diagnostic> {
        self.aliases.truncate(usize::from(self.depth));
        for position in 0..self.aliases.len() {
            match self.aliases[position].take() {
                Some(Alias::Local(local)) => {
                    self.emit_emission(wasm_emit::move_register(position as Register, local))?
                }
                Some(Alias::I32(value)) => self.load_i32(position as Register, value)?,
                Some(Alias::I64(value)) => self.load_scalar(position as Register, WasmValue::I64(i64::from(value)))?,
                None => {}
            }
        }
        Ok(())
    }

    /// Operators that address the operand stack only through push/pop, so an
    /// aliased operand may be read directly from its local register.
    fn reads_aliases(&self, operator: &Operator<'_>) -> bool {
        matches!(
            operator,
            Operator::LocalGet { .. }
                | Operator::LocalSet { .. }
                | Operator::LocalTee { .. }
                | Operator::Drop
                | Operator::Nop
                | Operator::I32Const { .. }
                | Operator::I64Const { .. }
                | Operator::F32Const { .. }
                | Operator::F64Const { .. }
                | Operator::If { .. }
                | Operator::BrIf { .. }
                | Operator::BrTable { .. }
                | Operator::Select
                | Operator::TypedSelect { .. }
        ) || matches!(
            operator,
            Operator::GlobalGet { global_index } | Operator::GlobalSet { global_index }
                if self.global_register(*global_index).is_some()
        ) || (numeric_operator(operator).is_some() && !integer::wide_integer(operator))
            || simd::reads_aliases(operator) || reference::reads_aliases(operator)
            || memory::direct_access(operator).is_some_and(|(_, memarg)| {
                matches!(
                    self.memory_registers.get(memarg.memory as usize),
                    Some(Some(_))
                )
            })
    }

    /// Materialize positions that alias `local` before the local is overwritten.
    fn materialize_local_aliases(&mut self, local: Register) -> Result<(), Diagnostic> {
        for position in 0..usize::from(self.depth).min(self.aliases.len()) {
            if self.aliases[position] == Some(Alias::Local(local)) {
                self.aliases[position] = None;
                self.emit_emission(wasm_emit::move_register(position as Register, local))?;
            }
        }
        Ok(())
    }

    fn push_alias(&mut self, alias: Alias) -> Result<(), Diagnostic> {
        let position = usize::from(self.push()?);
        if self.aliases.len() <= position {
            self.aliases.resize(position + 1, None);
        }
        self.aliases[position] = Some(alias);
        Ok(())
    }

    /// Record that the last emitted instruction wrote the operand-stack top.
    fn produced(&mut self, result: Register) {
        self.producer = Some((self.code.len() - 1, result));
    }

    /// The previous operator's last instruction, when it alone wrote `value`
    /// and nothing has been emitted since: it may be rewritten in place.
    pub(super) fn previous_producer(&self, value: Register) -> Option<usize> {
        self.previous
            .filter(|(pc, result)| *result == value && pc + 1 == self.code.len())
            .map(|(pc, _)| pc)
    }

    pub(super) fn instruction(&self, pc: usize) -> WideInstruction {
        crate::bytecode::widen(self.code[pc], &self.wide)
    }

    /// Write `value` into `local`: redirect its producer when it was the
    /// immediately preceding instruction, otherwise copy it.
    fn store_register_local(&mut self, local: Register) -> Result<(), Diagnostic> {
        if let Some(value) = self.top_i32_constant() {
            let position = self.pop_position()?;
            self.take_alias(position);
            self.materialize_local_aliases(local)?;
            return self.load_i32(local, value);
        }
        if let Some(value) = self.top_i64_constant() {
            let position = self.pop_position()?;
            self.take_alias(position);
            self.materialize_local_aliases(local)?;
            return self.load_scalar(local, WasmValue::I64(i64::from(value)));
        }
        let value = self.pop()?;
        let emitted = self.code.len();
        self.materialize_local_aliases(local)?;
        if value == local {
            return Ok(());
        }
        if self.code.len() == emitted
            && let Some(pc) = self.previous_producer(value)
        {
            let mut instruction = self.instruction(pc);
            instruction.set_result_register(local);
            self.code.pop();
            return self.emit(
                instruction.op(),
                instruction.a(),
                instruction.b(),
                instruction.c(),
                instruction.imm(),
            );
        }
        self.emit_emission(wasm_emit::move_register(local, value))
    }

    /// An i32 operator in its first-class form; a constant right operand
    /// becomes the immediate.
    /// The last emitted instruction, when no label lies at or after it and
    /// the next instruction may therefore replace it.
    pub(super) fn fusable_last(&self) -> Option<(usize, crate::bytecode::WideInstruction)> {
        let pc = self.code.len().checked_sub(1)?;
        (pc >= self.fusion_floor).then(|| (pc, self.instruction(pc)))
    }

    /// Whether `register` holds a Wasm local rather than an operand-stack value.
    pub(super) fn is_local_register(&self, register: Register) -> bool {
        self.local_base
            .is_some_and(|base| (base..base + self.locals).contains(&register))
    }

    /// The shift of `(x >>> k) & m` when it was emitted last and its result
    /// is the stack value the mask consumes: its source and amount.
    fn take_bit_field_shift(&mut self, shifted: Register) -> Option<(Register, u16)> {
        let (_, shift) = self.fusable_last()?;
        if !wasm_emit::is_immediate_of(shift, I32BinaryOperator::ShiftRightUnsigned)
            || shift.a() != shifted
            || self.is_local_register(shifted)
        {
            return None;
        }
        // Wasm takes shift amounts modulo the width; others keep two operators.
        let amount = u16::try_from(shift.imm())
            .ok()
            .filter(|amount| u32::from(*amount) < i32::BITS)?;
        self.code.pop();
        Some((shift.b(), amount))
    }

    fn i32_binary(&mut self, operator: I32BinaryOperator) -> Result<(), Diagnostic> {
        let constant = self.top_i32_constant();
        let right = match constant {
            Some(_) => {
                let position = self.pop_position()?;
                self.take_alias(position);
                0
            }
            None => self.pop()?,
        };
        let left = self.pop()?;
        let result = self.push()?;
        let emission = match constant {
            Some(value) => {
                // A logical shift under a constant mask fuses into one bit-field extract.
                let bit_field = (operator == I32BinaryOperator::And)
                    .then(|| self.take_bit_field_shift(left))
                    .flatten();
                match bit_field {
                    Some((source, amount)) => {
                        wasm_emit::bit_field_extract(result, source, amount, value as u32)
                    }
                    None => wasm_emit::i32_binary_immediate(operator, result, left, value as u32),
                }
            }
            None => wasm_emit::i32_binary(operator, result, left, right),
        };
        self.emit_emission(emission)?;
        self.produced(result);
        Ok(())
    }

    /// Load a defaultable local type's default value.
    fn load_default(
        &mut self,
        result: Register,
        ty: wasmparser::ValType,
    ) -> Result<(), Diagnostic> {
        let constant = self.default_constant(ty)?;
        self.emit_emission(wasm_emit::load_constant(result, constant))
    }

    fn default_constant(&mut self, ty: wasmparser::ValType) -> Result<u32, Diagnostic> {
        let constant = match ty {
            wasmparser::ValType::Ref(_) => Constant::Null,
            wasmparser::ValType::I32 | wasmparser::ValType::F32 => return Ok(ZERO_LOCAL_CONSTANT),
            ty => WasmType::from_wasm(ty)
                .and_then(|ty| ty.default_value())
                .and_then(|value| value.constant())
                .ok_or_else(|| Diagnostic::unsupported(self.name, "non-defaultable Wasm local"))?,
        };
        self.append_constant(constant)
    }

    /// Give register locals their defaults with one fill per run of locals that
    /// share a default. A local whose first use is an unconditional top-level
    /// `local.set` or `local.tee` is always written before it is read.
    fn fill_register_locals(
        &mut self,
        base: Register,
        params: u16,
        locals: &[wasmparser::ValType],
        assigned_first: &[bool],
    ) -> Result<(), Diagnostic> {
        let mut run: Option<(Register, u16, u32)> = None;
        for (index, ty) in locals.iter().copied().enumerate() {
            let register = base + params + index as Register;
            // Activations start every register at i32 zero (see
            // `InitialRegister`), which is already the i32 and f32 default.
            let starts_at_default =
                matches!(ty, wasmparser::ValType::I32 | wasmparser::ValType::F32);
            let constant = if ty.is_defaultable() && !assigned_first[index] && !starts_at_default {
                Some(self.default_constant(ty)?)
            } else {
                None
            };
            match (run, constant) {
                (Some((start, count, value)), Some(constant))
                    if value == constant && start + count == register =>
                {
                    run = Some((start, count + 1, value));
                }
                (_, constant) => {
                    if let Some((start, count, value)) = run {
                        self.emit_emission(wasm_emit::fill_registers(start, count, value))?;
                    }
                    run = constant.map(|constant| (register, 1, constant));
                }
            }
        }
        if let Some((start, count, value)) = run {
            self.emit_emission(wasm_emit::fill_registers(start, count, value))?;
        }
        Ok(())
    }

    fn load_i32(&mut self, result: Register, value: i32) -> Result<(), Diagnostic> {
        self.load_scalar(result, WasmValue::I32(value))
    }

    fn append_constant(&mut self, value: Constant) -> Result<u32, Diagnostic> {
        self.constants.intern(self.settled_constants, value).ok_or_else(|| Diagnostic::unsupported(self.name, "too many Wasm constants"))
    }

    fn scalar_constant(&mut self, value: WasmValue) -> Result<u32, Diagnostic> {
        self.append_constant(value.constant().ok_or_else(|| {
            Diagnostic::unsupported(
                self.name,
                "non-null reference is not a static scalar constant",
            )
        })?)
    }

    fn load_scalar(&mut self, result: Register, value: WasmValue) -> Result<(), Diagnostic> {
        let constant = self.scalar_constant(value)?;
        self.emit_emission(wasm_emit::load_constant(result, constant))
    }

    fn emit_wasm_call(
        &mut self,
        params: u16,
        results: u16,
        target: WasmCallTarget,
        tail: bool,
    ) -> Result<(), Diagnostic> {
        let base = self
            .depth
            .checked_sub(params)
            .filter(|base| *base >= self.control_base())
            .ok_or_else(|| {
                Diagnostic::unsupported(self.name, "Wasm call argument stack underflow")
            })?;
        let immediate = ImmediateLayout::call_immediate(base, params, false, false);
        if ImmediateLayout::call_window_base(immediate) != base {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm call exceeds argument window layout",
            ));
        }
        let receiver = match target {
            WasmCallTarget::Known(_) => 0,
            WasmCallTarget::Reference(reference) => {
                self.depth = self.depth.max(reference + 1);
                let receiver = self.push()?;
                self.emit_emission(wasm_emit::load_constant(receiver, VOID_RESULT_CONSTANT))?;
                receiver
            }
        };
        self.depth = base;
        let result = self.push()?;
        let destination = if tail {
            result | crate::bytecode::RETURN_REGISTER
        } else {
            result
        };
        let call = match target {
            WasmCallTarget::Known(function) => {
                wasm_emit::call_known(destination, function, immediate)
            }
            WasmCallTarget::Reference(reference) => {
                wasm_emit::call_through(destination, reference, receiver, immediate)
            }
        };
        self.emit_emission(call)?;
        if tail {
            self.make_dead();
            return Ok(());
        }
        self.depth = base;
        for _ in 0..results {
            self.push()?;
        }
        if usize::from(results) > SCALAR_RETURN_ARITY {
            let bundle = self.push()?;
            self.emit_emission(wasm_emit::move_register(bundle, result))?;
            let index = self.push()?;
            for offset in 0..results {
                self.load_scalar(index, WasmValue::I32(i32::from(offset)))?;
                self.emit_emission(wasm_emit::get_index(base + offset, bundle, index))?;
            }
            self.depth = base + results;
        }
        Ok(())
    }

    fn operator(&mut self, operator: Operator<'_>) -> Result<(), Diagnostic> {
        self.previous = self.producer.take();
        if !self.reads_aliases(&operator) {
            self.materialize_aliases()?;
        }
        if self.control_operator(&operator)? {
            // Control operators bind labels at the current position.
            self.fusion_floor = self.code.len();
            return Ok(());
        }
        if self.element_operator(&operator)? {
            return Ok(());
        }
        if self.table_operator(&operator)? {
            return Ok(());
        }
        if self.gc_operator(&operator)?
            || self.reference_operator(&operator)?
            || self.wide_integer_operator(&operator)?
            || self.memory_operator(&operator)?
            || self.simd_operator(&operator)?
        {
            return Ok(());
        }
        if let Some(op) = i31::I31Operator::from_wasm(&operator) {
            if self.path == Reachability::Live {
                let input = self.pop()?;
                let result = self.push()?;
                self.emit_emission(wasm_emit::i31_operation(result, input, op))?;
            }
            return Ok(());
        }
        if let Some(numeric) = numeric_operator(&operator) {
            if self.path == Reachability::Dead {
                return Ok(());
            }
            let (family, selector) = match numeric {
                NumericLowering::I32Binary(operator) => return self.i32_binary(operator),
                NumericLowering::Selector(family, selector) => (family, selector),
            };
            if family == SelectorFamily::I64Binary
                && let Some(value) = self.top_i64_constant()
            {
                let position = self.pop_position()?;
                self.take_alias(position);
                let left = self.pop()?;
                let result = self.push()?;
                self.emit_emission(wasm_emit::i64_binary_immediate(result, left, value, selector))?;
                self.produced(result);
                return Ok(());
            }
            let right = if family.takes_right() { self.pop()? } else { 0 };
            let left = self.pop()?;
            let result = self.push()?;
            self.emit_emission(family.emit(result, left, right, selector))?;
            self.produced(result);
            return Ok(());
        }
        match operator {
            Operator::I32Const { value } => {
                if self.path == Reachability::Live {
                    self.push_alias(Alias::I32(value))?;
                }
                Ok(())
            }
            Operator::I64Const { value } if i16::try_from(value).is_ok() => {
                if self.path == Reachability::Live {
                    self.push_alias(Alias::I64(value as i16))?;
                }
                Ok(())
            }
            Operator::I64Const { .. }
            | Operator::F32Const { .. }
            | Operator::F64Const { .. }
            | Operator::V128Const { .. } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let result = self.push()?;
                let value = match operator {
                    Operator::I64Const { value } => WasmValue::I64(value),
                    Operator::F32Const { value } => WasmValue::F32(value.bits()),
                    Operator::F64Const { value } => WasmValue::F64(value.bits()),
                    Operator::V128Const { value } => {
                        WasmValue::V128(u128::from_le_bytes(*value.bytes()))
                    }
                    _ => unreachable!(),
                };
                self.load_scalar(result, value)
            }
            Operator::GlobalGet { global_index } | Operator::GlobalSet { global_index } => {
                let global = self.globals.get(global_index as usize).ok_or_else(|| {
                    Diagnostic::unsupported(self.name, "Wasm global out of bounds")
                })?;
                let write = matches!(operator, Operator::GlobalSet { .. });
                if write && !global.mutable {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "write to immutable Wasm global",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                if let Some(binding) = self.global_register(global_index) {
                    if write {
                        let value = self.pop()?;
                        return self.emit_emission(wasm_emit::global_set(value, binding));
                    }
                    let result = self.push()?;
                    self.emit_emission(wasm_emit::global_get(result, binding))?;
                    self.produced(result);
                    return Ok(());
                }
                let register = if write { self.pop()? } else { self.push()? };
                let depth = self.depth;
                // Keep the popped write operand live while loading the owner.
                if write {
                    self.depth += 1;
                }
                let binding = self.instance_binding(global_index as usize)?;
                let emission = if write {
                    wasm_emit::global_set(register, binding)
                } else {
                    wasm_emit::global_get(register, binding)
                };
                self.emit_emission(emission)?;
                self.depth = depth;
                Ok(())
            }
            Operator::LocalGet { local_index }
            | Operator::LocalSet { local_index }
            | Operator::LocalTee { local_index } => {
                if local_index >= u32::from(self.locals) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm local out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                if let Some(base) = self.local_base {
                    let local = base + local_index as Register;
                    if !matches!(operator, Operator::LocalGet { .. }) {
                        self.store_register_local(local)?;
                    }
                    if !matches!(operator, Operator::LocalSet { .. }) {
                        self.push_alias(Alias::Local(local))?;
                    }
                    return Ok(());
                }
                // The bound above keeps the local index within the local count, a u16.
                let slot = local_index as u16;
                let emission = match operator {
                    Operator::LocalGet { .. } => wasm_emit::load_local(self.push()?, slot),
                    Operator::LocalSet { .. } => wasm_emit::store_local(self.pop()?, slot),
                    Operator::LocalTee { .. } => {
                        let register = self.pop()?;
                        self.push()?;
                        wasm_emit::store_local(register, slot)
                    }
                    _ => unreachable!(),
                };
                self.emit_emission(emission)
            }
            Operator::Call { function_index } | Operator::ReturnCall { function_index } => {
                let signature = self
                    .signatures
                    .get(function_index as usize)
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm call target out of bounds")
                    })?;
                let params = u16::try_from(signature.params.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm call parameters")
                })?;
                let results = u16::try_from(signature.results.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm call results")
                })?;
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let target = match self.signatures.defined_index(function_index) {
                    Some(index) => WasmCallTarget::Known(u16::try_from(index).map_err(|_| {
                        Diagnostic::unsupported(
                            self.name,
                            "Wasm call target exceeds function index layout",
                        )
                    })?),
                    None => {
                        let reference = self.push()?;
                        self.emit_emission(wasm_emit::ref_func(reference, function_index))?;
                        // The imported target is scratch, outside the argument window.
                        self.depth -= 1;
                        WasmCallTarget::Reference(reference)
                    }
                };
                self.emit_wasm_call(
                    params,
                    results,
                    target,
                    matches!(operator, Operator::ReturnCall { .. }),
                )
            }
            Operator::Drop if self.path == Reachability::Live => self.pop().map(drop),
            Operator::Drop => Ok(()),
            Operator::Nop => Ok(()),
            operator => Err(Diagnostic::unsupported(
                self.name,
                format!("unsupported shared Wasm operator: {operator:?}"),
            )),
        }
    }
}
