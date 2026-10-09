//! Wasm operator lowering into the shared residual vocabulary. Binary decoding
//! and module validation belong to quench-wasm, not this execution core.

use crate::bytecode::{
    AtomTable, Constant, DispatchClass, Function, ImmediateLayout, Instr, Op, ProgramKind,
    Register, ResidualProgram, WideInstruction,
};
use crate::{Diagnostic, Engine};
use std::rc::Rc;
use wasmparser::{BinaryReaderError, Operator};

mod constant;
pub(crate) mod gc;
pub use constant::WasmConstantExpression;
pub(crate) use constant::{WasmArrayInitializer, WasmInitializerEffect};
pub(crate) mod tag;
pub use tag::WasmTag;
mod element;
pub use element::{WasmElement, WasmElementMode};
pub use table::{WasmTable, WasmTableInitializer};
pub(crate) mod atomic;
pub(crate) mod memory;
mod wait;
pub(crate) const INDEX_GROW_FAILURE: i64 = -1;

pub use memory::{WasmData, WasmDataMode, WasmMemory};
mod control;
mod scalar;
mod types;
pub use types::WasmTypes;
pub(crate) mod simd;
pub(crate) mod table;
pub(crate) use scalar::{ScalarBits, V128_BYTES, WasmSignatures};
pub use scalar::{
    WasmCallableType, WasmFunctionBody, WasmFunctionImport, WasmReferenceKind, WasmSignature,
    WasmType, WasmValue,
};
pub(crate) mod conversion;
pub(crate) mod float;
pub(crate) mod i31;
pub(crate) mod integer;
mod numeric;
pub(crate) mod reference;
use control::{Control, Reachability};
use conversion::ScalarConversionOperator;
use float::{F32BinaryOperator, F32UnaryOperator, F64BinaryOperator, F64UnaryOperator};
use integer::{I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator};

/// A WebAssembly trap, distinct from a JavaScript throw or invalid residual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmTrap {
    Unreachable,
    CallStackExhausted,
    IntegerDivideByZero,
    IntegerOverflow,
    InvalidConversionToInteger,
    OutOfBoundsMemory,
    UnalignedAtomic,
    WaitOnUnsharedMemory,
    TooManyWaiters,
    OutOfBoundsTable,
    OutOfBoundsArray,
    UndefinedElement,
    UninitializedElement,
    NullReference,
    NullFunctionReference,
    NullI31Reference,
    NullStructReference,
    NullArrayReference,
    NullExceptionReference,
    NullDescriptorReference,
    ArrayTooLarge,
    CastFailure,
    DescriptorCastFailure,
    IndirectCallTypeMismatch,
}

impl std::fmt::Display for WasmTrap {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str(match self {
            Self::UndefinedElement => "undefined element",
            Self::UninitializedElement => "uninitialized element",
            Self::NullReference => "null reference",
            Self::NullFunctionReference => "null function reference",
            Self::NullArrayReference => "null array reference",
            Self::NullExceptionReference => "null exception reference",
            Self::NullDescriptorReference => "null descriptor reference",
            Self::ArrayTooLarge => "array too large",
            Self::NullStructReference => "null structure reference",
            Self::NullI31Reference => "null i31 reference",
            Self::CastFailure => "cast failure",
            Self::DescriptorCastFailure => "descriptor cast failure",
            Self::IndirectCallTypeMismatch => "indirect call type mismatch",
            Self::OutOfBoundsArray => "out of bounds array access",
            Self::OutOfBoundsTable => "out of bounds table access",
            Self::WaitOnUnsharedMemory => "atomic wait on unshared memory",
            Self::TooManyWaiters => "too many atomic waiters",
            Self::UnalignedAtomic => "unaligned atomic",
            Self::OutOfBoundsMemory => "out of bounds memory access",
            Self::Unreachable => "unreachable",
            Self::CallStackExhausted => "call stack exhausted",
            Self::IntegerDivideByZero => "integer divide by zero",
            Self::IntegerOverflow => "integer overflow",
            Self::InvalidConversionToInteger => "invalid conversion to integer",
        })
    }
}

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

impl WasmReferenceInitializer {
    fn validate(
        &self,
        name: &str,
        ty: wasmparser::RefType,
        signatures: &WasmSignatures,
        globals: &[WasmGlobal],
    ) -> Result<(), Diagnostic> {
        match self {
            Self::Null if ty.is_nullable() => Ok(()),
            Self::Function(index)
                if signatures.function_type_index(*index as usize).is_some_and(
                    |actual| match ty.heap_type() {
                        wasmparser::HeapType::FUNC => true,
                        wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(
                            index,
                        ))
                        | wasmparser::HeapType::Exact(wasmparser::UnpackedIndex::Module(index)) => {
                            signatures.matches_declaration(
                                actual,
                                &signatures.declarations,
                                index,
                                matches!(ty.heap_type(), wasmparser::HeapType::Exact(_)),
                            )
                        }
                        _ => false,
                    },
                ) =>
            {
                Ok(())
            }
            Self::Expression(expression)
                if signatures
                    .declarations
                    .callable_value_type(wasmparser::ValType::Ref(ty))
                    .is_some_and(|target| {
                        signatures.declarations.value_subtype(
                            expression.value_type(),
                            &signatures.declarations,
                            target,
                        )
                    })
                    || (ty.heap_type() == wasmparser::HeapType::FUNC
                        && expression.produces_non_null_function()) =>
            {
                expression.validate(name, globals)?;
                expression.validate_functions(name, signatures, globals)
            }
            _ => Err(Diagnostic::unsupported(
                name,
                "invalid Wasm reference initializer",
            )),
        }
    }
}

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
fn import_names<'a>(
    functions: &'a [scalar::CallableImport],
    globals: &'a [WasmGlobal],
    memories: &'a [WasmMemory],
    tables: &'a [WasmTable],
    tags: &'a [WasmTag],
) -> impl Iterator<Item = &'a WasmImportName> {
    functions
        .iter()
        .map(|import| &import.name)
        .chain(globals.iter().filter_map(WasmGlobal::import))
        .chain(memories.iter().filter_map(|memory| memory.import.as_ref()))
        .chain(tables.iter().filter_map(|table| match &table.initializer {
            WasmTableInitializer::Import(name) => Some(name),
            WasmTableInitializer::Reference(_) => None,
        }))
        .chain(tags.iter().filter_map(|tag| tag.import.as_ref()))
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
        let mut imports: Vec<_> = import_names(
            self.signatures.imports(),
            &self.globals,
            &self.memories,
            &self.tables,
            &self.tags,
        )
        .collect();
        imports.sort_by_key(|name| name.index);
        imports
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
        operators: impl IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
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
        for tag in tags {
            tag.validate(name, types)?;
        }
        let (function_types, bodies): (Vec<_>, Vec<_>) = bodies
            .into_iter()
            .map(|body| (body.ty, (body.locals, body.operators)))
            .unzip();
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
            if let WasmElementMode::Active { table, offset } = &element.mode {
                let table = tables.get(*table as usize).ok_or_else(|| {
                    Diagnostic::unsupported(name, "Wasm element table out of bounds")
                })?;
                offset.validate(name, globals)?;
                if Some(offset.value_type()) != WasmType::from_wasm(table.ty.index_type()) {
                    return Err(Diagnostic::unsupported(
                        name,
                        "invalid Wasm element offset type",
                    ));
                }

                if !types.reference_subtype(element.element_type, types, table.ty.element_type) {
                    return Err(Diagnostic::unsupported(
                        name,
                        "incompatible Wasm element type",
                    ));
                }
            }
            for item in element.items.iter() {
                item.validate(name, element.element_type, &signature_pool, globals)?;
            }
        }
        for table in tables {
            if let WasmTableInitializer::Reference(initializer) = &table.initializer {
                initializer.validate(name, table.ty.element_type, &signature_pool, globals)?;
            }
        }
        for segment in data {
            if u32::try_from(segment.bytes.len()).is_err() {
                return Err(Diagnostic::unsupported(
                    name,
                    "Wasm data segment exceeds index domain",
                ));
            }
            if let WasmDataMode::Active { memory, offset } = &segment.mode {
                offset.validate(name, globals)?;
                let memory = memories.get(*memory as usize).ok_or_else(|| {
                    Diagnostic::unsupported(name, "Wasm data memory index out of bounds")
                })?;
                if Some(offset.value_type()) != WasmType::from_wasm(memory.ty.index_type()) {
                    return Err(Diagnostic::unsupported(
                        name,
                        "invalid Wasm data offset type",
                    ));
                }
            }
        }
        for (index, global) in globals.iter().enumerate() {
            if let WasmGlobalInitializer::Expression(expression) = &global.initial {
                expression.validate(name, &globals[..index])?;
                expression.validate_functions(name, &signature_pool, &globals[..index])?;
            }
        }
        let mut import_indices: Vec<_> =
            import_names(signature_pool.imports(), globals, memories, tables, tags)
                .map(|name| name.index)
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
        let mut constants = vec![Constant::Number(0.0), Constant::Undefined];
        let mut functions = Vec::with_capacity(bodies.len());
        for (index, (locals, operators)) in bodies.into_iter().enumerate() {
            let index = u32::try_from(index)
                .map_err(|_| Diagnostic::unsupported(name, "too many Wasm functions"))?;
            let signature = signature_pool.defined_signature(index).unwrap().clone();
            let params = u16::try_from(signature.params.len())
                .map_err(|_| Diagnostic::unsupported(name, "too many Wasm parameters"))?;
            let results = u16::try_from(signature.results.len())
                .map_err(|_| Diagnostic::unsupported(name, "too many Wasm function results"))?;
            let error = |message: &str| Diagnostic::unsupported(name, message);
            let local_count = usize::from(params)
                .checked_add(locals.len())
                .and_then(|count| u16::try_from(count).ok())
                .ok_or_else(|| error("too many Wasm locals"))?;
            let mut lowering = Lowering {
                name,
                locals: local_count,
                temporary_locals: 0,
                code: Vec::new(),
                wide: Vec::new(),
                constants,
                signatures: &mut signature_pool,
                globals,
                memories,
                data,
                tables,
                elements,
                tags,
                handlers: vec![],
                depth: 0,
                registers: 1,
                controls: vec![Control::function(results)],
                path: Reachability::Live,
            };
            // Defaultable locals get residual defaults; others remain uninitialized.
            // The frontend proves assignment before non-defaultable local reads.
            for (index, ty) in locals.into_iter().enumerate() {
                if ty.is_defaultable() {
                    match ty {
                        wasmparser::ValType::Ref(_) => {
                            let constant = lowering.append_constant(Constant::Null)?;
                            lowering.emit(Op::LoadConst, 0, 0, 0, constant)?;
                        }
                        _ => lowering
                            .load_zero(0, WasmType::from_wasm(ty).expect("numeric local type"))?,
                    }
                    let slot = usize::from(params) + index;
                    lowering.emit(Op::StoreLocal, 0, 0, 0, slot as u32)?;
                }
            }
            for operator in operators {
                let operator =
                    operator.map_err(|e| Diagnostic::unsupported(name, e.to_string()))?;
                if lowering.controls.is_empty() {
                    return Err(error("operators after Wasm function end"));
                }
                lowering.operator(operator)?;
            }
            if !lowering.controls.is_empty() {
                return Err(error("missing Wasm function end"));
            }
            let function = Function {
                parent: None,
                name: None,
                is_arrow: false,
                self_binding_slot: None,
                source_text: None,
                params,
                length: params,
                parameter_end_pc: 0,
                parameter_atoms: vec![],
                rest: false,
                is_async: false,
                is_generator: false,
                is_class_constructor: false,
                derived_constructor: false,
                instance_initializer: None,
                super_home_atom: None,
                constructible: false,
                class_field_initializer: false,
                parameter_eval_arguments_error: false,
                arguments_slot: None,
                simple_parameters: true,
                strict: true,
                locals: lowering.locals + lowering.temporary_locals,
                local_atoms: vec![],
                environment_atoms: vec![],
                selective_capture_slots: None,
                inherited_with_scope: false,
                lexical_atoms: vec![],
                global_lexical_atoms: vec![],
                global_var_atoms: vec![],
                global_function_atoms: vec![],
                global_annex_b_var_atoms: vec![],
                global_immutable_atoms: vec![],
                name_bindings: vec![],
                binding_sites: vec![],
                source_positions: vec![],
                environment_clones: vec![],
                code: lowering.code,
                wide: lowering.wide,
                registers: lowering.registers,
                dispatch: DispatchClass::General,
                decoded: Default::default(),
                handlers: lowering.handlers,
                register_root_offset: crate::bytecode::NO_REGISTER_ROOT_MAP,
            };
            functions.push(function);
            constants = lowering.constants;
        }
        let register_roots = crate::compile::liveness::derive(&mut functions, &[], &[], &[]);
        let program = ResidualProgram {
            specialized: false,
            kind: ProgramKind::Wasm,
            module_requests: vec![],
            module_imports: vec![],
            module_link_plan: None,
            source_name: name.into(),
            atoms: AtomTable::default(),
            constants,
            functions,
            cache_sites: 0,
            method_sites: vec![],
            method_arguments: vec![],
            field_sites: vec![],
            object_sites: vec![],
            regexp_literal_sites: vec![],
            superinstructions: vec![],
            register_roots,
        };
        program
            .validate()
            .map_err(|e| Diagnostic::unsupported(name, e))?;
        Ok(WasmModule {
            program: Rc::new(program),
            signatures: Rc::new(signature_pool),
            globals: globals.into(),
            memories: memories.into(),
            data: data.into(),
            tables: tables.into(),
            elements: elements.into(),
            tags: tags.into(),
            start: None,
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
                Operator::I32Add,
                Op::WasmI32Binary,
                WasmType::I32,
                WasmType::I32,
            ),
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
            assert!(function.module.program.validate().is_err());
            let error = crate::Runtime::new(crate::SystemHost)
                .execute_wasm(&function, &[ty.default_value().unwrap()])
                .unwrap_err();
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

struct Lowering<'a> {
    signatures: &'a mut WasmSignatures,
    globals: &'a [WasmGlobal],
    memories: &'a [WasmMemory],
    data: &'a [WasmData],
    tables: &'a [WasmTable],
    elements: &'a [WasmElement],
    tags: &'a [WasmTag],
    handlers: Vec<crate::bytecode::Handler>,
    name: &'a str,
    locals: u16,
    temporary_locals: u16,
    code: Vec<Instr>,
    wide: Vec<WideInstruction>,
    constants: Vec<Constant>,
    depth: Register,
    registers: u16,
    controls: Vec<Control>,
    path: Reachability,
}

enum WasmCallTarget {
    Known(u16),
    Reference(Register),
}

impl Lowering<'_> {
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
        Ok(register)
    }

    fn pop(&mut self) -> Result<Register, Diagnostic> {
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

    fn load_i32(&mut self, result: Register, value: i32) -> Result<(), Diagnostic> {
        self.load_scalar(result, WasmValue::I32(value))
    }

    fn load_zero(&mut self, result: Register, ty: WasmType) -> Result<(), Diagnostic> {
        if matches!(ty, WasmType::FUNCREF | WasmType::EXTERNREF) {
            return self.load_scalar(result, ty.default_value().unwrap());
        }
        if matches!(ty, WasmType::I32 | WasmType::F32) {
            return self.emit(Op::LoadConst, result, 0, 0, ZERO_LOCAL_CONSTANT);
        }
        if matches!(ty, WasmType::I64 | WasmType::F64)
            && let Some(index) = self
                .constants
                .iter()
                .position(|constant| matches!(constant, Constant::WasmBits64(0)))
        {
            return self.emit(Op::LoadConst, result, 0, 0, index as u32);
        }
        self.load_scalar(result, ty.default_value().unwrap())
    }

    fn append_constant(&mut self, value: Constant) -> Result<u32, Diagnostic> {
        let constant = u32::try_from(self.constants.len())
            .map_err(|_| Diagnostic::unsupported(self.name, "too many Wasm constants"))?;
        self.constants.push(value);
        Ok(constant)
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
        self.emit(Op::LoadConst, result, 0, 0, constant)
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
        let (op, callee, receiver) = match target {
            WasmCallTarget::Known(function) => (Op::CallKnown, function, 0),
            WasmCallTarget::Reference(reference) => {
                self.depth = self.depth.max(reference + 1);
                let receiver = self.push()?;
                self.emit(Op::LoadConst, receiver, 0, 0, VOID_RESULT_CONSTANT)?;
                (Op::Call, reference, receiver)
            }
        };
        self.depth = base;
        let result = self.push()?;
        let destination = if tail {
            result | crate::bytecode::RETURN_REGISTER
        } else {
            result
        };
        self.emit(op, destination, callee, receiver, immediate)?;
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
            self.emit(Op::Move, bundle, result, 0, 0)?;
            let index = self.push()?;
            for offset in 0..results {
                self.load_scalar(index, WasmValue::I32(i32::from(offset)))?;
                self.emit(
                    Op::GetIndex,
                    base + offset,
                    crate::bytecode::Operand::register(bundle).0,
                    crate::bytecode::Operand::register(index).0,
                    0,
                )?;
            }
            self.depth = base + results;
        }
        Ok(())
    }

    fn operator(&mut self, operator: Operator<'_>) -> Result<(), Diagnostic> {
        if self.control_operator(&operator)? {
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
                self.emit(Op::WasmI31, result, input, 0, op as u32)?;
            }
            return Ok(());
        }
        let numeric = I32BinaryOperator::from_wasm(&operator)
            .map(|op| (Op::WasmI32Binary, op as u32))
            .or_else(|| {
                I32UnaryOperator::from_wasm(&operator).map(|op| (Op::WasmI32Unary, op as u32))
            })
            .or_else(|| {
                I64BinaryOperator::from_wasm(&operator).map(|op| (Op::WasmI64Binary, op as u32))
            })
            .or_else(|| {
                I64UnaryOperator::from_wasm(&operator).map(|op| (Op::WasmI64Unary, op as u32))
            });
        let numeric = numeric.or_else(|| {
            ScalarConversionOperator::from_wasm(&operator)
                .map(|op| (Op::WasmScalarConvert, op as u32))
        });
        let numeric = numeric
            .or_else(|| {
                F32BinaryOperator::from_wasm(&operator).map(|op| (Op::WasmF32Binary, op as u32))
            })
            .or_else(|| {
                F32UnaryOperator::from_wasm(&operator).map(|op| (Op::WasmF32Unary, op as u32))
            })
            .or_else(|| {
                F64BinaryOperator::from_wasm(&operator).map(|op| (Op::WasmF64Binary, op as u32))
            })
            .or_else(|| {
                F64UnaryOperator::from_wasm(&operator).map(|op| (Op::WasmF64Unary, op as u32))
            });
        if let Some((op, selector)) = numeric {
            if self.path == Reachability::Dead {
                return Ok(());
            }
            let right = if op
                .field_layout(crate::bytecode::InstructionField::C)
                .is_register_field()
            {
                self.pop()?
            } else {
                0
            };
            let left = self.pop()?;
            let result = self.push()?;
            return self.emit(op, result, left, right, selector);
        }
        match operator {
            Operator::I32Const { .. }
            | Operator::I64Const { .. }
            | Operator::F32Const { .. }
            | Operator::F64Const { .. }
            | Operator::V128Const { .. } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let result = self.push()?;
                let value = match operator {
                    Operator::I32Const { value } => WasmValue::I32(value),
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
                let register = if write { self.pop()? } else { self.push()? };
                let depth = self.depth;
                // Keep the popped write operand live while loading the owner.
                if write {
                    self.depth += 1;
                }
                let binding = self.instance_binding(global_index as usize)?;
                self.emit(
                    if write {
                        Op::WasmGlobalSet
                    } else {
                        Op::WasmGlobalGet
                    },
                    register,
                    binding,
                    0,
                    0,
                )?;
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
                let (op, register) = match operator {
                    Operator::LocalGet { .. } => (Op::LoadLocal, self.push()?),
                    Operator::LocalSet { .. } => (Op::StoreLocal, self.pop()?),
                    Operator::LocalTee { .. } => {
                        let register = self.pop()?;
                        self.push()?;
                        (Op::StoreLocal, register)
                    }
                    _ => unreachable!(),
                };
                self.emit(op, register, 0, 0, local_index)
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
                        self.emit(Op::WasmRefFunc, reference, 0, 0, function_index)?;
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
