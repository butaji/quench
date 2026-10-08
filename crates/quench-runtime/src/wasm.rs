//! Wasm operator lowering into the shared residual vocabulary. Binary decoding
//! and module validation belong to quench-wasm, not this execution core.

use crate::bytecode::{
    AtomTable, Constant, DispatchClass, Function, ImmediateLayout, Instr, Op, ProgramKind,
    Register, ResidualProgram, WideInstruction,
};
use crate::{Diagnostic, Engine};
use wasmparser::{BinaryReaderError, Operator};

pub(crate) mod atomic;
mod control;
mod scalar;
pub(crate) use scalar::ScalarBits;
pub use scalar::{WasmFunctionBody, WasmGcField, WasmGcType, WasmSignature, WasmType, WasmValue};
pub(crate) mod conversion;
pub(crate) mod float;
pub(crate) mod integer;
mod numeric;
pub(crate) mod simd;
pub(crate) mod wide;
use control::{Control, Reachability};
use conversion::ScalarConversionOperator;
use float::{F32BinaryOperator, F32UnaryOperator, F64BinaryOperator, F64UnaryOperator};
use integer::{I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator};
use wide::WideArithmeticOperator;

/// A WebAssembly trap, distinct from a JavaScript throw or invalid residual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmTrap {
    Unreachable,
    CallStackExhausted,
    IntegerDivideByZero,
    IntegerOverflow,
    InvalidConversionToInteger,
    MemoryOutOfBounds,
    UnalignedAtomic,
    UndefinedElement,
    IndirectCallTypeMismatch,
    TableOutOfBounds,
    NullReference,
    NullI31Reference,
    NullFunctionReference,
    NullArrayReference,
    NullStructReference,
    NullDescriptorReference,
    ArrayOutOfBounds,
    CastFailure,
}

impl std::fmt::Display for WasmTrap {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str(match self {
            Self::Unreachable => "unreachable",
            Self::CallStackExhausted => "call stack exhausted",
            Self::IntegerDivideByZero => "integer divide by zero",
            Self::IntegerOverflow => "integer overflow",
            Self::InvalidConversionToInteger => "invalid conversion to integer",
            Self::MemoryOutOfBounds => "out of bounds memory access",
            Self::UnalignedAtomic => "unaligned atomic access",
            Self::UndefinedElement => "undefined element (uninitialized element)",
            Self::IndirectCallTypeMismatch => "indirect call type mismatch",
            Self::TableOutOfBounds => "out of bounds table access",
            Self::NullReference => "null reference",
            Self::NullI31Reference => "null i31 reference",
            Self::NullFunctionReference => "null function reference",
            Self::NullArrayReference => "null array reference",
            Self::NullStructReference => "null structure reference",
            Self::NullDescriptorReference => "null descriptor reference",
            Self::ArrayOutOfBounds => "out of bounds array access",
            Self::CastFailure => "cast failure",
        })
    }
}

const ZERO_LOCAL_CONSTANT: u32 = 0;
const VOID_RESULT_CONSTANT: u32 = 1;
pub(crate) const WASM_GC_ALLOC_KIND_BITS: u32 = 4;
pub(crate) const WASM_GC_ALLOC_KIND_MASK: u32 = (1 << WASM_GC_ALLOC_KIND_BITS) - 1;
pub(crate) const WASM_GC_ALLOC_STRUCT: u32 = 0;
pub(crate) const WASM_GC_ALLOC_STRUCT_DEFAULT: u32 = 1;
pub(crate) const WASM_GC_ALLOC_ARRAY_DEFAULT: u32 = 2;
pub(crate) const WASM_GC_ALLOC_ARRAY: u32 = 3;
pub(crate) const WASM_GC_ALLOC_ARRAY_FIXED: u32 = 4;
pub(crate) const WASM_GC_ALLOC_ARRAY_DATA: u32 = 5;
pub(crate) const WASM_GC_ALLOC_ARRAY_ELEM: u32 = 6;
pub(crate) const WASM_GC_ALLOC_STRUCT_DESC: u32 = 7;
pub(crate) const WASM_GC_ALLOC_STRUCT_DEFAULT_DESC: u32 = 8;
pub(crate) const WASM_GC_ALLOC_SEGMENT_TYPE_SHIFT: u32 = 16;
pub(crate) const WASM_GC_ALLOC_SEGMENT_INDEX_SHIFT: u32 = WASM_GC_ALLOC_KIND_BITS;
pub(crate) const WASM_GC_ALLOC_SEGMENT_INDEX_MASK: u32 =
    (1 << (WASM_GC_ALLOC_SEGMENT_TYPE_SHIFT - WASM_GC_ALLOC_SEGMENT_INDEX_SHIFT)) - 1;
pub(crate) const WASM_GC_ACCESS_SELECTOR_SHIFT: u32 = 28;
pub(crate) const WASM_GC_ACCESS_FIELD_MASK: u32 = (1 << WASM_GC_ACCESS_SELECTOR_SHIFT) - 1;
pub(crate) const WASM_GC_ACCESS_STRUCT_GET: u32 = 0;
pub(crate) const WASM_GC_ACCESS_STRUCT_GET_S: u32 = 1;
pub(crate) const WASM_GC_ACCESS_STRUCT_GET_U: u32 = 2;
pub(crate) const WASM_GC_ACCESS_ARRAY_GET: u32 = 3;
pub(crate) const WASM_GC_ACCESS_ARRAY_GET_S: u32 = 4;
pub(crate) const WASM_GC_ACCESS_ARRAY_GET_U: u32 = 5;
pub(crate) const WASM_GC_ACCESS_STRUCT_SET: u32 = 6;
pub(crate) const WASM_GC_ACCESS_ARRAY_SET: u32 = 7;
pub(crate) const WASM_GC_ACCESS_ARRAY_LEN: u32 = 8;
pub(crate) const WASM_GC_ACCESS_ARRAY_FILL: u32 = 9;
pub(crate) const WASM_GC_ACCESS_ARRAY_COPY: u32 = 10;
pub(crate) const WASM_GC_ACCESS_ARRAY_INIT_DATA: u32 = 11;
pub(crate) const WASM_GC_ACCESS_ARRAY_INIT_ELEM: u32 = 12;
pub(crate) const WASM_GC_ACCESS_REGISTER_BITS: u32 = 14;
pub(crate) const WASM_GC_ACCESS_REGISTER_MASK: u32 = (1 << WASM_GC_ACCESS_REGISTER_BITS) - 1;
pub(crate) const WASM_GC_ACCESS_SEGMENT_SHIFT: u32 = WASM_GC_ACCESS_REGISTER_BITS;
pub(crate) const WASM_GC_ACCESS_SEGMENT_MASK: u32 = WASM_GC_ACCESS_REGISTER_MASK;
pub(crate) const WASM_GC_ACCESS_SECOND_REGISTER_SHIFT: u32 = WASM_GC_ACCESS_REGISTER_BITS;
pub const WASM_GC_REFERENCE_CLASS_SHIFT: u32 = 30;
pub(crate) const WASM_GC_REFERENCE_ID_MASK: u32 = (1 << WASM_GC_REFERENCE_CLASS_SHIFT) - 1;
pub const WASM_GC_REFERENCE_STRUCT_CLASS: u32 = 2;
pub const WASM_GC_REFERENCE_ARRAY_CLASS: u32 = 3;
pub const WASM_GC_INITIAL_REFERENCE_TAG: u32 = 0xf000_0000;
pub const WASM_GC_INITIAL_REFERENCE_ID_MASK: u32 = 0x0fff_ffff;
pub(crate) const WASM_EXTERNREF_TAG: u32 = 0x1000_0000;
pub(crate) const WASM_ANYREF_EXTERN_TAG: u32 = 0x2000_0000;
pub(crate) const WASM_REFERENCE_HANDLE_MASK: u32 = 0x0fff_ffff;
pub(crate) const WASM_FUNCTION_REF_HANDLE_TAG: u32 = 0x0f00_0000;
pub(crate) const WASM_FUNCTION_REF_HANDLE_ID_MASK: u32 = 0x00ff_ffff;

/// A shared residual program containing all functions from one Wasm module.
pub struct WasmFunction {
    pub(crate) program: ResidualProgram,
    pub(crate) signatures: Vec<WasmSignature>,
    pub(crate) entry: u32,
    pub(crate) globals: Vec<(WasmValue, bool)>,
    pub(crate) memories: Vec<WasmMemoryInit>,
    pub(crate) memory_imports: Vec<Option<WasmMemoryId>>,
    pub(crate) tables: Vec<WasmTableInit>,
    pub(crate) table_imports: Vec<Option<WasmTableId>>,
    pub(crate) element_segments: Vec<WasmElementSegment>,
    pub(crate) imported_functions: Vec<Option<WasmFunctionRef>>,
    pub(crate) tag_signatures: Vec<WasmSignature>,
    pub(crate) tag_imports: Vec<Option<WasmTagId>>,
    pub(crate) exception_handlers: Vec<Vec<WasmExceptionHandler>>,
    pub(crate) start_function: Option<u32>,
    pub(crate) v128_sites: Vec<WasmV128Site>,
    pub(crate) atomic_sites: Vec<WasmAtomicSite>,
    pub(crate) type_signatures: Vec<WasmSignature>,
    pub(crate) gc_supertypes: Vec<Option<u32>>,
    pub(crate) gc_type_canonicals: Vec<u32>,
    pub(crate) gc_type_fingerprints: Vec<String>,
    pub(crate) gc_descriptors: Vec<WasmGcDescriptor>,
    pub(crate) function_type_indices: Vec<u32>,
    pub(crate) gc_types: Vec<Option<WasmGcType>>,
    pub(crate) gc_initial_objects: Vec<WasmGcInitialObject>,
    pub(crate) indirect_sites: Vec<WasmIndirectSite>,
    pub(crate) data_segments: Vec<Option<Vec<u8>>>,
}

/// A GC object materialized while evaluating a module constant expression.
#[derive(Clone, Debug)]
pub struct WasmGcInitialObject {
    pub reference: u32,
    pub type_index: u32,
    pub values: Vec<WasmValue>,
    pub descriptor: Option<WasmValue>,
}

/// Descriptor relationships attached to one module-local GC type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WasmGcDescriptor {
    pub descriptor_type: Option<u32>,
    pub describes_type: Option<u32>,
}

/// Initial state for one memory in a decoded module.
#[derive(Clone, Debug)]
pub struct WasmMemoryInit {
    pub initial_pages: u64,
    pub maximum_pages: Option<u64>,
    pub memory64: bool,
    pub shared: bool,
    pub page_size_log2: u32,
    pub data_segments: Vec<(u64, Vec<u8>)>,
}

/// Handle for a memory shared by modules retained in one runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WasmMemoryId(u32);

impl WasmMemoryId {
    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }
}

/// Initial function-reference table state retained by one Wasm instance.
#[derive(Clone, Debug)]
pub struct WasmTableInit {
    pub initial_size: u64,
    pub maximum_size: Option<u64>,
    pub table64: bool,
    pub element_type: WasmType,
    pub initial_value: WasmValue,
    pub elements: Vec<(u64, Vec<WasmValue>)>,
}

#[derive(Clone, Debug)]
pub struct WasmElementSegment {
    pub element_type: WasmType,
    pub values: Option<Vec<WasmValue>>,
}

/// A function import bound to another module retained by the same runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WasmFunctionRef {
    pub module: WasmModuleId,
    pub function_index: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WasmV128Site {
    pub operator: u16,
    pub lane: Option<u8>,
    pub shuffle: Option<[u8; 16]>,
    pub memory_index: Option<u32>,
    pub offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WasmAtomicSite {
    pub action: atomic::WasmAtomicAction,
    pub value_type: Option<WasmType>,
    pub access_width: u8,
    pub memory_index: u32,
    pub offset: u64,
}

impl WasmAtomicSite {
    fn from_operator(operator: &Operator<'_>) -> Option<Self> {
        let site = atomic::WasmAtomicSite::from_operator(operator)?;
        Some(Self {
            action: site.action,
            value_type: site.value_type,
            access_width: site.access_width,
            memory_index: site.memory_index,
            offset: site.offset,
        })
    }

    pub(crate) const fn input_count(self) -> u16 {
        match self.action {
            atomic::WasmAtomicAction::Load => 1,
            atomic::WasmAtomicAction::Store | atomic::WasmAtomicAction::Notify => 2,
            atomic::WasmAtomicAction::Wait => 3,
            atomic::WasmAtomicAction::Rmw(atomic::AtomicRmw::CompareExchange) => 3,
            atomic::WasmAtomicAction::Rmw(_) => 2,
            atomic::WasmAtomicAction::Fence => 0,
        }
    }

    const fn has_result(self) -> bool {
        !matches!(
            self.action,
            atomic::WasmAtomicAction::Store | atomic::WasmAtomicAction::Fence
        )
    }
}

/// A table shared by modules retained in one runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WasmTableId(u32);

impl WasmTableId {
    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WasmIndirectSite {
    pub table_index: u32,
    pub type_index: u32,
}

/// Scalar memory instruction encoding shared by lowering and dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum WasmMemoryAccessKind {
    I32Load,
    I64Load,
    F32Load,
    F64Load,
    I32Load8S,
    I32Load8U,
    I32Load16S,
    I32Load16U,
    I64Load8S,
    I64Load8U,
    I64Load16S,
    I64Load16U,
    I64Load32S,
    I64Load32U,
    I32Store,
    I64Store,
    F32Store,
    F64Store,
    I32Store8,
    I32Store16,
    I64Store8,
    I64Store16,
    I64Store32,
}

impl WasmMemoryAccessKind {
    pub(crate) fn from_tag(tag: u16) -> Option<Self> {
        use WasmMemoryAccessKind::*;
        Some(match tag {
            0 => I32Load,
            1 => I64Load,
            2 => F32Load,
            3 => F64Load,
            4 => I32Load8S,
            5 => I32Load8U,
            6 => I32Load16S,
            7 => I32Load16U,
            8 => I64Load8S,
            9 => I64Load8U,
            10 => I64Load16S,
            11 => I64Load16U,
            12 => I64Load32S,
            13 => I64Load32U,
            14 => I32Store,
            15 => I64Store,
            16 => F32Store,
            17 => F64Store,
            18 => I32Store8,
            19 => I32Store16,
            20 => I64Store8,
            21 => I64Store16,
            22 => I64Store32,
            _ => return None,
        })
    }

    pub(crate) const fn is_store(self) -> bool {
        (self as u16) >= Self::I32Store as u16
    }

    pub(crate) const fn width(self) -> usize {
        match self {
            Self::I32Load8S
            | Self::I32Load8U
            | Self::I64Load8S
            | Self::I64Load8U
            | Self::I32Store8
            | Self::I64Store8 => 1,
            Self::I32Load16S
            | Self::I32Load16U
            | Self::I64Load16S
            | Self::I64Load16U
            | Self::I32Store16
            | Self::I64Store16 => 2,
            Self::I32Load
            | Self::F32Load
            | Self::I64Load32S
            | Self::I64Load32U
            | Self::I32Store
            | Self::F32Store
            | Self::I64Store32 => 4,
            Self::I64Load | Self::F64Load | Self::I64Store | Self::F64Store => 8,
        }
    }

    pub(crate) const fn value_type(self) -> WasmType {
        match self {
            Self::I32Load
            | Self::I32Load8S
            | Self::I32Load8U
            | Self::I32Load16S
            | Self::I32Load16U
            | Self::I32Store
            | Self::I32Store8
            | Self::I32Store16 => WasmType::I32,
            Self::I64Load
            | Self::I64Load8S
            | Self::I64Load8U
            | Self::I64Load16S
            | Self::I64Load16U
            | Self::I64Load32S
            | Self::I64Load32U
            | Self::I64Store
            | Self::I64Store8
            | Self::I64Store16
            | Self::I64Store32 => WasmType::I64,
            Self::F32Load | Self::F32Store => WasmType::F32,
            Self::F64Load | Self::F64Store => WasmType::F64,
        }
    }

    pub(crate) fn from_operator(operator: &Operator<'_>) -> Option<(Self, u32, u32)> {
        use Operator::*;
        let (kind, memarg) = match operator {
            I32Load { memarg } => (Self::I32Load, memarg),
            I64Load { memarg } => (Self::I64Load, memarg),
            F32Load { memarg } => (Self::F32Load, memarg),
            F64Load { memarg } => (Self::F64Load, memarg),
            I32Load8S { memarg } => (Self::I32Load8S, memarg),
            I32Load8U { memarg } => (Self::I32Load8U, memarg),
            I32Load16S { memarg } => (Self::I32Load16S, memarg),
            I32Load16U { memarg } => (Self::I32Load16U, memarg),
            I64Load8S { memarg } => (Self::I64Load8S, memarg),
            I64Load8U { memarg } => (Self::I64Load8U, memarg),
            I64Load16S { memarg } => (Self::I64Load16S, memarg),
            I64Load16U { memarg } => (Self::I64Load16U, memarg),
            I64Load32S { memarg } => (Self::I64Load32S, memarg),
            I64Load32U { memarg } => (Self::I64Load32U, memarg),
            I32Store { memarg } => (Self::I32Store, memarg),
            I64Store { memarg } => (Self::I64Store, memarg),
            F32Store { memarg } => (Self::F32Store, memarg),
            F64Store { memarg } => (Self::F64Store, memarg),
            I32Store8 { memarg } => (Self::I32Store8, memarg),
            I32Store16 { memarg } => (Self::I32Store16, memarg),
            I64Store8 { memarg } => (Self::I64Store8, memarg),
            I64Store16 { memarg } => (Self::I64Store16, memarg),
            I64Store32 { memarg } => (Self::I64Store32, memarg),
            _ => return None,
        };
        Some((kind, u32::try_from(memarg.offset).ok()?, memarg.memory))
    }
}

/// Compatibility name for the i32-only lowering and execution boundaries.
pub type WasmI32Function = WasmFunction;
/// A decoded Wasm module lowered into shared VM bytecode.
pub type WasmModule = WasmFunction;

/// Handle for a Wasm module retained by one shared runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WasmModuleId(u32);

impl WasmModuleId {
    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }
}

/// Identity of a WebAssembly exception tag shared by linked modules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WasmTagId {
    pub(crate) module: WasmModuleId,
    pub(crate) index: u32,
}

/// Handle for a scalar global retained by one shared runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WasmGlobalId(u32);

impl WasmGlobalId {
    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug)]
pub(crate) struct WasmExceptionHandler {
    pub start: u32,
    pub end: u32,
    pub tag_index: Option<u32>,
    pub target: u32,
    pub payload_register: u16,
    pub payload_count: u16,
    pub catch_ref: bool,
    pub exception_register: Option<u16>,
}

impl WasmFunction {
    pub fn set_start_function(&mut self, function_index: u32) {
        self.start_function = Some(function_index);
    }

    /// Bind imported exception tags to identities from linked instances.
    #[doc(hidden)]
    pub fn set_tag_imports(&mut self, imports: Vec<Option<WasmTagId>>) {
        self.tag_imports = imports;
    }

    /// Attach the module-local type hierarchy decoded by the Wasm frontend.
    #[doc(hidden)]
    pub fn set_gc_type_metadata(
        &mut self,
        supertypes: Vec<Option<u32>>,
        canonicals: Vec<u32>,
        fingerprints: Vec<String>,
    ) {
        self.gc_supertypes = supertypes;
        self.gc_type_canonicals = canonicals;
        self.gc_type_fingerprints = fingerprints;
    }

    /// Attach module-local function type indices used by typed references.
    #[doc(hidden)]
    pub fn set_function_type_metadata(&mut self, indices: Vec<u32>) {
        self.function_type_indices = indices;
    }

    /// Attach the module-local GC field layouts used by shared execution.
    #[doc(hidden)]
    pub fn set_gc_layout_metadata(&mut self, gc_types: Vec<Option<WasmGcType>>) {
        self.gc_types = gc_types;
    }

    /// Attach descriptor relationships decoded from the module type section.
    #[doc(hidden)]
    pub fn set_gc_descriptor_metadata(&mut self, descriptors: Vec<WasmGcDescriptor>) {
        self.gc_descriptors = descriptors;
    }

    pub fn set_gc_initial_objects(&mut self, objects: Vec<WasmGcInitialObject>) {
        self.gc_initial_objects = objects;
    }

    /// Attach imported memory identities resolved by the Wasm linker.
    #[doc(hidden)]
    pub fn set_memory_imports(&mut self, imports: Vec<Option<WasmMemoryId>>) {
        self.memory_imports = imports;
    }

    pub fn residual(&self) -> &ResidualProgram {
        &self.program
    }
    pub fn signature(&self) -> &WasmSignature {
        &self.signatures[self.entry as usize]
    }

    pub fn function_signatures(&self) -> &[WasmSignature] {
        &self.signatures
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
                    signature: WasmSignature {
                        params: vec![WasmType::I32; usize::from(params)],
                        result: has_result.then_some(WasmType::I32),
                        additional_results: Vec::new(),
                    },
                    locals: vec![WasmType::I32; usize::from(locals)],
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
        Self::lower_wasm_module_with_globals(name, entry, bodies, &[])
    }

    /// Lower validated scalar functions that read immutable scalar globals.
    /// Mutable global access remains a runtime operation and is rejected until
    /// its storage is part of the shared VM instance state.
    pub fn lower_wasm_module_with_globals<'a, I>(
        name: &str,
        entry: u32,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        globals: &[(WasmValue, bool)],
    ) -> Result<WasmFunction, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
    {
        Self::lower_wasm_module_with_state(name, entry, bodies, globals, &[])
    }

    /// Lower scalar functions together with memory32 declarations and active
    /// data initializers from a validated module.
    pub fn lower_wasm_module_with_state<'a, I>(
        name: &str,
        entry: u32,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        globals: &[(WasmValue, bool)],
        memories: &[WasmMemoryInit],
    ) -> Result<WasmFunction, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
    {
        Self::lower_wasm_module_with_module_state(
            name,
            entry,
            bodies,
            globals,
            memories,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        )
    }

    /// Lower functions with all immutable module metadata needed by indirect
    /// calls and table initialization.
    pub fn lower_wasm_module_with_module_state<'a, I>(
        name: &str,
        entry: u32,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
        globals: &[(WasmValue, bool)],
        memories: &[WasmMemoryInit],
        tables: &[WasmTableInit],
        type_signatures: &[WasmSignature],
        gc_types: &[Option<WasmGcType>],
        element_segments: &[WasmElementSegment],
        data_segments: &[Option<Vec<u8>>],
        imported_functions: &[Option<WasmFunctionRef>],
        imported_tables: &[Option<WasmTableId>],
        tag_signatures: &[WasmSignature],
    ) -> Result<WasmFunction, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
    {
        let (signatures, bodies): (Vec<_>, Vec<_>) = bodies
            .into_iter()
            .map(|body| (body.signature, (body.locals, body.operators)))
            .unzip();
        if entry as usize >= signatures.len() {
            return Err(Diagnostic::unsupported(
                name,
                "Wasm entry function out of bounds",
            ));
        }
        let mut constants = vec![Constant::Number(0.0), Constant::Undefined];
        let mut functions = Vec::with_capacity(bodies.len());
        let mut indirect_sites = Vec::new();
        let mut v128_sites = Vec::new();
        let mut atomic_sites = Vec::new();
        let mut exception_handlers = Vec::with_capacity(bodies.len());
        for (signature, (locals, operators)) in signatures.iter().zip(bodies) {
            let params = u16::try_from(signature.params.len())
                .map_err(|_| Diagnostic::unsupported(name, "too many Wasm parameters"))?;
            let result_count = u16::try_from(signature.result_count())
                .map_err(|_| Diagnostic::unsupported(name, "too many Wasm results"))?;
            let local_types: Vec<_> = signature.params.iter().copied().chain(locals).collect();
            let error = |message: &str| Diagnostic::unsupported(name, message);
            let local_count =
                u16::try_from(local_types.len()).map_err(|_| error("too many Wasm locals"))?;
            let mut lowering = Lowering {
                name,
                locals: local_count,
                code: Vec::new(),
                wide: Vec::new(),
                constants,
                signatures: &signatures,
                globals,
                memories,
                tables,
                type_signatures,
                tag_signatures,
                exception_handlers: Vec::new(),
                delegated_regions: Vec::new(),
                exception_registers: Vec::new(),
                rethrow_sites: Vec::new(),
                gc_types,
                indirect_sites: &mut indirect_sites,
                v128_sites: &mut v128_sites,
                atomic_sites: &mut atomic_sites,
                data_segment_count: data_segments.len(),
                element_segment_count: element_segments.len(),
                depth: 0,
                registers: 1,
                controls: vec![Control::function(result_count)],
                path: Reachability::Live,
            };
            // Shared JS frames initialize non-parameter locals to undefined. Wasm
            // initialization is therefore explicit residual code, not another frame.
            for (slot, ty) in local_types.iter().enumerate().skip(usize::from(params)) {
                lowering.load_zero(0, *ty)?;
                lowering.emit(Op::StoreLocal, 0, 0, 0, slot as u32)?;
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
            lowering.relocate_exception_registers()?;
            exception_handlers.push(std::mem::take(&mut lowering.exception_handlers));
            let function = Function {
                parent: None,
                name: None,
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
                locals: local_count,
                local_atoms: vec![],
                environment_atoms: vec![],
                lexical_atoms: vec![],
                global_lexical_atoms: vec![],
                global_var_atoms: vec![],
                global_function_atoms: vec![],
                global_annex_b_var_atoms: vec![],
                global_immutable_atoms: vec![],
                binding_sites: vec![],
                name_bindings: vec![],
                environment_clones: vec![],
                code: lowering.code,
                wide: lowering.wide,
                registers: lowering.registers,
                dispatch: DispatchClass::General,
                handlers: vec![],
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
            superinstructions: vec![],
            register_roots,
        };
        program
            .validate()
            .map_err(|e| Diagnostic::unsupported(name, e))?;
        Ok(WasmFunction {
            program,
            signatures,
            entry,
            globals: globals.to_vec(),
            memories: memories.to_vec(),
            memory_imports: Vec::new(),
            tables: tables.to_vec(),
            table_imports: imported_tables.to_vec(),
            element_segments: element_segments.to_vec(),
            imported_functions: imported_functions.to_vec(),
            tag_signatures: tag_signatures.to_vec(),
            tag_imports: vec![None; tag_signatures.len()],
            exception_handlers,
            start_function: None,
            v128_sites,
            atomic_sites,
            type_signatures: type_signatures.to_vec(),
            gc_supertypes: Vec::new(),
            gc_type_canonicals: Vec::new(),
            gc_type_fingerprints: Vec::new(),
            gc_descriptors: vec![WasmGcDescriptor::default(); gc_types.len()],
            function_type_indices: Vec::new(),
            gc_types: gc_types.to_vec(),
            gc_initial_objects: Vec::new(),
            indirect_sites,
            data_segments: data_segments.to_vec(),
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
                    signature: WasmSignature {
                        params: vec![ty],
                        result: Some(result_type),
                        additional_results: Vec::new(),
                    },
                    locals: vec![],
                    operators: operators.into_iter().map(Ok),
                }],
            )
            .unwrap();
            let residual = &mut function.program.functions[0];
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
            assert!(function.program.validate().is_err());
            let error = crate::Runtime::new(crate::SystemHost)
                .execute_wasm(&function, &[ty.zero()])
                .unwrap_err();
            assert_eq!(error.wasm_trap(), None);
        }
    }

    #[test]
    fn scalar_conversion_rows_own_input_output_types_and_selector_domain() {
        assert_eq!(ScalarConversionOperator::from_tag(u32::MAX), None);
        for operator in ScalarConversionOperator::ALL {
            let input = operator.source_type().zero();
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
                signature: WasmSignature {
                    params: vec![WasmType::F64; 2],
                    result: Some(WasmType::F64),
                    additional_results: Vec::new(),
                },
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
        function.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.program = decoded.unwrap();
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
                signature: WasmSignature {
                    params: vec![WasmType::I64; 2],
                    result: Some(WasmType::I64),
                    additional_results: Vec::new(),
                },
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
        function.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.program = decoded.unwrap();
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
                signature: WasmSignature {
                    params: vec![],
                    result: Some(WasmType::I64),
                    additional_results: Vec::new(),
                },
                locals: vec![],
                operators: [Operator::I64Const { value: bits as i64 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            }],
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-shared-scalars-{}.qbc", std::process::id()));
        function.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.program = decoded.unwrap();
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
        function.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.program = decoded.unwrap();
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
    signatures: &'a [WasmSignature],
    globals: &'a [(WasmValue, bool)],
    memories: &'a [WasmMemoryInit],
    tables: &'a [WasmTableInit],
    type_signatures: &'a [WasmSignature],
    tag_signatures: &'a [WasmSignature],
    gc_types: &'a [Option<WasmGcType>],
    indirect_sites: &'a mut Vec<WasmIndirectSite>,
    v128_sites: &'a mut Vec<WasmV128Site>,
    atomic_sites: &'a mut Vec<WasmAtomicSite>,
    data_segment_count: usize,
    element_segment_count: usize,
    name: &'a str,
    locals: u16,
    code: Vec<Instr>,
    wide: Vec<WideInstruction>,
    constants: Vec<Constant>,
    depth: Register,
    registers: u16,
    controls: Vec<Control>,
    path: Reachability,
    exception_handlers: Vec<WasmExceptionHandler>,
    delegated_regions: Vec<control::DelegatedRegion>,
    exception_registers: Vec<Register>,
    rethrow_sites: Vec<(usize, Register)>,
}

fn reference_type(heap_type: wasmparser::HeapType) -> Result<WasmType, Diagnostic> {
    match heap_type {
        wasmparser::HeapType::Abstract {
            ty: wasmparser::AbstractHeapType::Func | wasmparser::AbstractHeapType::NoFunc,
            ..
        }
        | wasmparser::HeapType::Concrete(_)
        | wasmparser::HeapType::Exact(_) => Ok(WasmType::FuncRef),
        wasmparser::HeapType::Abstract {
            ty: wasmparser::AbstractHeapType::Cont | wasmparser::AbstractHeapType::NoCont,
            ..
        } => Err(Diagnostic::unsupported(
            "Wasm",
            "unsupported Wasm reference type",
        )),
        wasmparser::HeapType::Abstract {
            ty: wasmparser::AbstractHeapType::I31,
            ..
        } => Ok(WasmType::I31Ref),
        wasmparser::HeapType::Abstract {
            ty: wasmparser::AbstractHeapType::Exn | wasmparser::AbstractHeapType::NoExn,
            ..
        } => Ok(WasmType::ExnRef),
        wasmparser::HeapType::Abstract { .. } => Ok(WasmType::ExternRef),
    }
}

fn reference_heap_code(heap_type: wasmparser::HeapType) -> Result<u32, Diagnostic> {
    match heap_type {
        wasmparser::HeapType::Abstract { ty, .. } => Ok(match ty {
            wasmparser::AbstractHeapType::Func => 1,
            wasmparser::AbstractHeapType::Extern => 2,
            wasmparser::AbstractHeapType::Any => 3,
            wasmparser::AbstractHeapType::None => 4,
            wasmparser::AbstractHeapType::NoExtern => 5,
            wasmparser::AbstractHeapType::NoFunc => 6,
            wasmparser::AbstractHeapType::Eq => 7,
            wasmparser::AbstractHeapType::Struct => 8,
            wasmparser::AbstractHeapType::Array => 9,
            wasmparser::AbstractHeapType::I31 => 10,
            wasmparser::AbstractHeapType::Exn => 11,
            wasmparser::AbstractHeapType::NoExn => 12,
            wasmparser::AbstractHeapType::Cont => 13,
            wasmparser::AbstractHeapType::NoCont => 14,
        }),
        wasmparser::HeapType::Concrete(index) => index
            .as_module_index()
            .map(|index| 0x8000_0000 | index)
            .ok_or_else(|| Diagnostic::unsupported("Wasm", "unsupported recursive heap type")),
        wasmparser::HeapType::Exact(index) => index
            .as_module_index()
            .map(|index| 0xa000_0000 | index)
            .ok_or_else(|| Diagnostic::unsupported("Wasm", "unsupported recursive heap type")),
    }
}

impl Lowering<'_> {
    fn relocate_exception_registers(&mut self) -> Result<(), Diagnostic> {
        for old_register in std::mem::take(&mut self.exception_registers) {
            let new_register = self.alloc_register()?;
            for handler in &mut self.exception_handlers {
                if handler.exception_register == Some(old_register) {
                    handler.exception_register = Some(new_register);
                }
            }
            for (site, register) in &mut self.rethrow_sites {
                if *register != old_register {
                    continue;
                }
                *register = new_register;
                let instruction = &mut self.code[*site];
                if instruction.is_wide() {
                    self.wide[instruction.wide_index()].set_b(new_register);
                } else {
                    instruction.set_b(new_register);
                }
            }
        }
        Ok(())
    }

    fn emit_tail_call(
        &mut self,
        op: Op,
        b: Register,
        c: Register,
        immediate: u32,
    ) -> Result<(), Diagnostic> {
        self.emit(op, 0, b, c, immediate)?;
        let instruction = self
            .code
            .last()
            .copied()
            .ok_or_else(|| Diagnostic::unsupported(self.name, "missing Wasm tail call"))?;
        if instruction.is_wide() {
            self.wide[instruction.wide_index()].set_returns_from_frame();
        } else {
            self.code
                .last_mut()
                .expect("tail call instruction was just emitted")
                .set_returns_from_frame();
        }
        self.make_dead();
        Ok(())
    }

    fn reserve_call_result(
        &mut self,
        base: Register,
        result_count: u16,
    ) -> Result<Register, Diagnostic> {
        self.depth = base;
        for _ in 0..result_count {
            self.push()?;
        }
        if result_count > 1 || result_count == 0 {
            let result = self.push()?;
            if result_count == 0 {
                self.depth = base;
            }
            Ok(result)
        } else {
            Ok(base)
        }
    }

    fn finish_call_result(
        &mut self,
        base: Register,
        result_count: u16,
        result: Register,
    ) -> Result<(), Diagnostic> {
        if result_count > 1 {
            for index in 0..result_count {
                self.emit(
                    Op::WasmMultiValueGet,
                    base + index,
                    result,
                    0,
                    u32::from(index),
                )?;
            }
            self.depth = base + result_count;
        }
        Ok(())
    }

    fn lower_simd_operator(&mut self, operator: &Operator<'_>) -> Result<bool, Diagnostic> {
        let debug = format!("{operator:?}");
        let name = debug
            .split(|character: char| character.is_whitespace() || character == '{')
            .next()
            .unwrap_or_default();
        let memory_arg = match operator {
            Operator::V128Load { memarg }
            | Operator::V128Load8x8S { memarg }
            | Operator::V128Load8x8U { memarg }
            | Operator::V128Load16x4S { memarg }
            | Operator::V128Load16x4U { memarg }
            | Operator::V128Load32x2S { memarg }
            | Operator::V128Load32x2U { memarg }
            | Operator::V128Load8Splat { memarg }
            | Operator::V128Load16Splat { memarg }
            | Operator::V128Load32Splat { memarg }
            | Operator::V128Load64Splat { memarg }
            | Operator::V128Load32Zero { memarg }
            | Operator::V128Load64Zero { memarg }
            | Operator::V128Load8Lane { memarg, .. }
            | Operator::V128Load16Lane { memarg, .. }
            | Operator::V128Load32Lane { memarg, .. }
            | Operator::V128Load64Lane { memarg, .. }
            | Operator::V128Store { memarg }
            | Operator::V128Store8Lane { memarg, .. }
            | Operator::V128Store16Lane { memarg, .. }
            | Operator::V128Store32Lane { memarg, .. }
            | Operator::V128Store64Lane { memarg, .. } => Some(*memarg),
            _ => None,
        };
        if let Some(memarg) = memory_arg {
            let tag = simd::tag(name).ok_or_else(|| {
                Diagnostic::unsupported(self.name, "unsupported Wasm SIMD memory operator")
            })?;
            let memory_index = memarg.memory;
            if self.memories.get(memory_index as usize).is_none() {
                return Err(Diagnostic::unsupported(
                    self.name,
                    "Wasm memory index out of bounds",
                ));
            }
            if memory_index > u16::MAX as u32 {
                return Err(Diagnostic::unsupported(self.name, "too many Wasm memories"));
            }
            let offset = u32::try_from(memarg.offset)
                .map_err(|_| Diagnostic::unsupported(self.name, "Wasm memory offset too large"))?;
            if self.path == Reachability::Dead {
                return Ok(true);
            }
            let lane = debug
                .split_once("lane: ")
                .and_then(|(_, suffix)| {
                    suffix
                        .split(|character: char| !character.is_ascii_digit())
                        .next()
                })
                .and_then(|digits| digits.parse().ok());
            let site = u32::try_from(self.v128_sites.len())
                .map_err(|_| Diagnostic::unsupported(self.name, "too many Wasm SIMD sites"))?;
            self.v128_sites.push(WasmV128Site {
                operator: tag,
                lane,
                shuffle: None,
                memory_index: Some(memory_index),
                offset,
            });
            if name.starts_with("V128Store") {
                let value = self.pop()?;
                let address = self.pop()?;
                self.emit(Op::WasmV128MemoryStore, address, value, 0, site)?;
            } else {
                let vector = if name.ends_with("Lane") {
                    Some(self.pop()?)
                } else {
                    None
                };
                let address = self.pop()?;
                let result = self.push()?;
                self.emit(
                    Op::WasmV128MemoryLoad,
                    result,
                    address,
                    vector.unwrap_or(0),
                    site,
                )?;
            }
            return Ok(true);
        }
        let Some(tag) = simd::tag(name) else {
            return Ok(false);
        };
        if self.path == Reachability::Dead {
            return Ok(true);
        }
        if let Operator::V128Const { value } = operator {
            let result = self.push()?;
            self.load_scalar(result, WasmValue::V128(u128::from(*value)))?;
            return Ok(true);
        }
        let arity = simd::arity(name);
        let mut arguments = [0; 3];
        for index in (0..arity).rev() {
            arguments[index] = self.pop()?;
        }
        let result = self.push()?;
        let lane = debug
            .split_once("lane: ")
            .and_then(|(_, suffix)| {
                suffix
                    .split(|character: char| !character.is_ascii_digit())
                    .next()
            })
            .and_then(|digits| digits.parse().ok());
        let shuffle = match operator {
            Operator::I8x16Shuffle { lanes } => Some(*lanes),
            _ => None,
        };
        let site = u32::try_from(self.v128_sites.len())
            .map_err(|_| Diagnostic::unsupported(self.name, "too many Wasm SIMD sites"))?;
        self.v128_sites.push(WasmV128Site {
            operator: tag,
            lane,
            shuffle,
            memory_index: None,
            offset: 0,
        });
        let (left, right) = match arity {
            1 => (arguments[0], 0),
            2 => (arguments[0], arguments[1]),
            3 => (arguments[1], arguments[2]),
            _ => unreachable!("SIMD operator arity"),
        };
        self.emit(Op::WasmV128, result, left, right, site)?;
        Ok(true)
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

    fn gc_type(&self, type_index: u32) -> Result<&WasmGcType, Diagnostic> {
        self.gc_types
            .get(type_index as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| Diagnostic::unsupported(self.name, "Wasm GC type index out of bounds"))
    }

    fn gc_alloc_immediate(&self, type_index: u32, kind: u32) -> Result<u32, Diagnostic> {
        type_index
            .checked_mul(1 << WASM_GC_ALLOC_KIND_BITS)
            .and_then(|value| value.checked_add(kind))
            .ok_or_else(|| Diagnostic::unsupported(self.name, "GC type index is too large"))
    }

    fn gc_segment_alloc_immediate(
        &self,
        type_index: u32,
        segment_index: u32,
        kind: u32,
    ) -> Result<u32, Diagnostic> {
        if type_index > u16::MAX as u32 || segment_index > WASM_GC_ALLOC_SEGMENT_INDEX_MASK {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm GC segment allocation index is too large",
            ));
        }
        Ok((type_index << WASM_GC_ALLOC_SEGMENT_TYPE_SHIFT)
            | (segment_index << WASM_GC_ALLOC_SEGMENT_INDEX_SHIFT)
            | kind)
    }

    fn gc_access_immediate(&self, selector: u32, index: u32) -> Result<u32, Diagnostic> {
        if index > WASM_GC_ACCESS_FIELD_MASK {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm GC field index is too large",
            ));
        }
        Ok((selector << WASM_GC_ACCESS_SELECTOR_SHIFT) | index)
    }

    fn gc_access_register_immediate(
        &self,
        selector: u32,
        register: Register,
    ) -> Result<u32, Diagnostic> {
        if u32::from(register) > WASM_GC_ACCESS_REGISTER_MASK {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm GC register index is too large",
            ));
        }
        Ok((selector << WASM_GC_ACCESS_SELECTOR_SHIFT) | u32::from(register))
    }

    fn gc_access_pair_immediate(
        &self,
        selector: u32,
        first: Register,
        second: Register,
    ) -> Result<u32, Diagnostic> {
        if u32::from(first) > WASM_GC_ACCESS_REGISTER_MASK
            || u32::from(second) > WASM_GC_ACCESS_REGISTER_MASK
        {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm GC register index is too large",
            ));
        }
        Ok((selector << WASM_GC_ACCESS_SELECTOR_SHIFT)
            | (u32::from(second) << WASM_GC_ACCESS_SECOND_REGISTER_SHIFT)
            | u32::from(first))
    }

    fn gc_access_segment_immediate(
        &self,
        selector: u32,
        segment_index: u32,
        length_register: Register,
    ) -> Result<u32, Diagnostic> {
        if segment_index > WASM_GC_ACCESS_SEGMENT_MASK
            || u32::from(length_register) > WASM_GC_ACCESS_REGISTER_MASK
        {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm GC segment index is too large",
            ));
        }
        Ok((selector << WASM_GC_ACCESS_SELECTOR_SHIFT)
            | (segment_index << WASM_GC_ACCESS_SEGMENT_SHIFT)
            | u32::from(length_register))
    }

    fn alloc_register(&mut self) -> Result<Register, Diagnostic> {
        let register = self.registers;
        if register > crate::bytecode::REGISTER_MASK {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm register layout exhausted",
            ));
        }
        self.registers = self
            .registers
            .checked_add(1)
            .ok_or_else(|| Diagnostic::unsupported(self.name, "too many Wasm registers"))?;
        Ok(register)
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
        if matches!(ty, WasmType::I32 | WasmType::F32) {
            return self.emit(Op::LoadConst, result, 0, 0, ZERO_LOCAL_CONSTANT);
        }
        if let Some(index) = self
            .constants
            .iter()
            .position(|constant| matches!(constant, Constant::WasmBits64(0)))
        {
            return self.emit(Op::LoadConst, result, 0, 0, index as u32);
        }
        self.load_scalar(result, ty.zero())
    }

    fn load_scalar(&mut self, result: Register, value: WasmValue) -> Result<(), Diagnostic> {
        let constant = u32::try_from(self.constants.len())
            .map_err(|_| Diagnostic::unsupported(self.name, "too many Wasm constants"))?;
        self.constants.push(value.constant());
        self.emit(Op::LoadConst, result, 0, 0, constant)
    }

    fn operator(&mut self, operator: Operator<'_>) -> Result<(), Diagnostic> {
        if self.control_operator(&operator)? {
            return Ok(());
        }
        if let Operator::Throw { tag_index } = &operator {
            let tag_index = *tag_index;
            let tag = self.tag_signatures.get(tag_index as usize).ok_or_else(|| {
                Diagnostic::unsupported(self.name, "Wasm exception tag out of bounds")
            })?;
            let count = u16::try_from(tag.params.len()).map_err(|_| {
                Diagnostic::unsupported(self.name, "too many Wasm exception payload values")
            })?;
            if self.path == Reachability::Dead {
                return Ok(());
            }
            let base = if count == 0 {
                0
            } else {
                self.depth
                    .checked_sub(count)
                    .filter(|base| *base >= self.control_base())
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm exception payload underflow")
                    })?
            };
            self.emit(Op::WasmThrow, 0, base, 0, tag_index)?;
            self.make_dead();
            return Ok(());
        }
        if matches!(operator, Operator::ThrowRef) {
            if self.path == Reachability::Dead {
                return Ok(());
            }
            let exception = self.pop()?;
            self.emit(Op::WasmThrowRef, 0, exception, 0, 0)?;
            self.make_dead();
            return Ok(());
        }
        if let Some(site) = WasmAtomicSite::from_operator(&operator) {
            if self.path == Reachability::Dead
                || matches!(site.action, atomic::WasmAtomicAction::Fence)
            {
                return Ok(());
            }
            let base = self
                .depth
                .checked_sub(site.input_count())
                .filter(|base| *base >= self.control_base())
                .ok_or_else(|| {
                    Diagnostic::unsupported(self.name, "Wasm atomic operand underflow")
                })?;
            for _ in 0..site.input_count() {
                self.pop()?;
            }
            let final_atomic_register = base
                .checked_add(2)
                .filter(|register| *register <= crate::bytecode::REGISTER_MASK)
                .ok_or_else(|| {
                    Diagnostic::unsupported(self.name, "Wasm atomic register layout exhausted")
                })?;
            self.registers = self.registers.max(final_atomic_register + 1);
            let site_index = u32::try_from(self.atomic_sites.len())
                .map_err(|_| Diagnostic::unsupported(self.name, "too many Wasm atomic sites"))?;
            self.atomic_sites.push(site);
            self.emit(Op::WasmAtomic, base, base + 1, base + 2, site_index)?;
            self.depth = base + u16::from(site.has_result());
            return Ok(());
        }
        if let Some((kind, offset, memory_index)) = WasmMemoryAccessKind::from_operator(&operator) {
            if self.memories.get(memory_index as usize).is_none() {
                return Err(Diagnostic::unsupported(
                    self.name,
                    "Wasm memory index out of bounds",
                ));
            }
            if memory_index > 2047 {
                return Err(Diagnostic::unsupported(self.name, "too many Wasm memories"));
            }
            if self.path == Reachability::Dead {
                return Ok(());
            }
            let selector = (memory_index << 5) | kind as u32;
            let selector = u16::try_from(selector)
                .map_err(|_| Diagnostic::unsupported(self.name, "Wasm memory selector overflow"))?;
            if kind.is_store() {
                let value = self.pop()?;
                let address = self.pop()?;
                return self.emit(Op::WasmMemoryStore, value, address, selector, offset);
            }
            let address = self.pop()?;
            let result = self.push()?;
            return self.emit(Op::WasmMemoryLoad, result, address, selector, offset);
        }
        if self.lower_simd_operator(&operator)? {
            return Ok(());
        }
        if let Some(operator) = WideArithmeticOperator::from_wasm(&operator) {
            if self.path == Reachability::Dead {
                return Ok(());
            }
            let base = self
                .depth
                .checked_sub(operator.input_count())
                .filter(|base| *base >= self.control_base())
                .ok_or_else(|| {
                    Diagnostic::unsupported(self.name, "Wasm wide arithmetic operand underflow")
                })?;
            for _ in 0..operator.input_count() {
                self.pop()?;
            }
            self.emit(Op::WasmWideArithmetic, base, base, 0, operator as u32)?;
            self.depth = base.checked_add(2).ok_or_else(|| {
                Diagnostic::unsupported(self.name, "Wasm wide arithmetic stack overflow")
            })?;
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
            | Operator::RefNull { .. } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let result = self.push()?;
                let value = match operator {
                    Operator::I32Const { value } => WasmValue::I32(value),
                    Operator::I64Const { value } => WasmValue::I64(value),
                    Operator::F32Const { value } => WasmValue::F32(value.bits()),
                    Operator::F64Const { value } => WasmValue::F64(value.bits()),
                    Operator::RefNull { hty } => match reference_type(hty)? {
                        WasmType::FuncRef => WasmValue::FuncRef(None),
                        WasmType::ExternRef => WasmValue::ExternRef(None),
                        WasmType::I31Ref => WasmValue::I31Ref(None),
                        WasmType::ExnRef => WasmValue::ExnRef(None),
                        _ => unreachable!("reference heap types produce references"),
                    },
                    _ => unreachable!(),
                };
                self.load_scalar(result, value)
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
            Operator::Call { function_index } => {
                let signature = self
                    .signatures
                    .get(function_index as usize)
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm call target out of bounds")
                    })?;
                let params = u16::try_from(signature.params.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm call parameters")
                })?;
                let result_count = u16::try_from(signature.result_count()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm call results")
                })?;
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let target = u16::try_from(function_index).map_err(|_| {
                    Diagnostic::unsupported(
                        self.name,
                        "Wasm call target exceeds function index layout",
                    )
                })?;
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
                let result = self.reserve_call_result(base, result_count)?;
                self.emit(Op::CallKnown, result, target, 0, immediate)?;
                self.finish_call_result(base, result_count, result)
            }
            Operator::ReturnCall { function_index } => {
                let signature = self
                    .signatures
                    .get(function_index as usize)
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm tail-call target out of bounds")
                    })?;
                let params = u16::try_from(signature.params.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm tail-call parameters")
                })?;
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let target = u16::try_from(function_index).map_err(|_| {
                    Diagnostic::unsupported(
                        self.name,
                        "Wasm tail-call target exceeds function index layout",
                    )
                })?;
                let base = self
                    .depth
                    .checked_sub(params)
                    .filter(|base| *base >= self.control_base())
                    .ok_or_else(|| {
                        Diagnostic::unsupported(
                            self.name,
                            "Wasm tail-call argument stack underflow",
                        )
                    })?;
                let immediate = ImmediateLayout::call_immediate(base, params, false, false);
                self.emit_tail_call(Op::CallKnown, target, 0, immediate)
            }
            Operator::CallIndirect {
                type_index,
                table_index,
                ..
            } => {
                let signature = self
                    .type_signatures
                    .get(type_index as usize)
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm indirect call type out of bounds")
                    })?;
                if self
                    .tables
                    .get(table_index as usize)
                    .is_none_or(|table| table.element_type != WasmType::FuncRef)
                {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm indirect call table out of bounds",
                    ));
                }
                let params = u16::try_from(signature.params.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many indirect call parameters")
                })?;
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let table_slot = self.pop()?;
                let base = self
                    .depth
                    .checked_sub(params)
                    .filter(|base| *base >= self.control_base())
                    .ok_or_else(|| {
                        Diagnostic::unsupported(
                            self.name,
                            "Wasm indirect call argument stack underflow",
                        )
                    })?;
                let site = u16::try_from(self.indirect_sites.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm indirect call sites")
                })?;
                self.indirect_sites.push(WasmIndirectSite {
                    table_index,
                    type_index,
                });
                let result_count = u16::try_from(signature.result_count()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm indirect call results")
                })?;
                let result = self.reserve_call_result(base, result_count)?;
                let immediate = ImmediateLayout::call_immediate(base, params, false, false);
                self.emit(Op::WasmCallIndirect, result, table_slot, site, immediate)?;
                self.finish_call_result(base, result_count, result)
            }
            Operator::ReturnCallIndirect {
                type_index,
                table_index,
                ..
            } => {
                let signature = self
                    .type_signatures
                    .get(type_index as usize)
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm tail-call type out of bounds")
                    })?;
                if self
                    .tables
                    .get(table_index as usize)
                    .is_none_or(|table| table.element_type != WasmType::FuncRef)
                {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm tail-call table out of bounds",
                    ));
                }
                let params = u16::try_from(signature.params.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm tail-call parameters")
                })?;
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let table_slot = self.pop()?;
                let base = self
                    .depth
                    .checked_sub(params)
                    .filter(|base| *base >= self.control_base())
                    .ok_or_else(|| {
                        Diagnostic::unsupported(
                            self.name,
                            "Wasm tail-call argument stack underflow",
                        )
                    })?;
                let site = u16::try_from(self.indirect_sites.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm indirect call sites")
                })?;
                self.indirect_sites.push(WasmIndirectSite {
                    table_index,
                    type_index,
                });
                let immediate = ImmediateLayout::call_immediate(base, params, false, false);
                self.emit_tail_call(Op::WasmCallIndirect, table_slot, site, immediate)
            }
            Operator::CallRef { type_index } => {
                let signature = self
                    .type_signatures
                    .get(type_index as usize)
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm call_ref type out of bounds")
                    })?;
                let params = u16::try_from(signature.params.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm call_ref parameters")
                })?;
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let reference = self.pop()?;
                let base = self
                    .depth
                    .checked_sub(params)
                    .filter(|base| *base >= self.control_base())
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm call_ref argument stack underflow")
                    })?;
                let site = u16::try_from(self.indirect_sites.len())
                    .map_err(|_| Diagnostic::unsupported(self.name, "too many Wasm call sites"))?;
                self.indirect_sites.push(WasmIndirectSite {
                    table_index: u32::MAX,
                    type_index,
                });
                let result_count = u16::try_from(signature.result_count()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm call_ref results")
                })?;
                let result = self.reserve_call_result(base, result_count)?;
                let immediate = ImmediateLayout::call_immediate(base, params, false, false);
                self.emit(Op::WasmCallIndirect, result, reference, site, immediate)?;
                self.finish_call_result(base, result_count, result)
            }
            Operator::ReturnCallRef { type_index } => {
                let signature = self
                    .type_signatures
                    .get(type_index as usize)
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm tail-call type out of bounds")
                    })?;
                let params = u16::try_from(signature.params.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm tail-call parameters")
                })?;
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let reference = self.pop()?;
                let base = self
                    .depth
                    .checked_sub(params)
                    .filter(|base| *base >= self.control_base())
                    .ok_or_else(|| {
                        Diagnostic::unsupported(
                            self.name,
                            "Wasm tail-call argument stack underflow",
                        )
                    })?;
                let site = u16::try_from(self.indirect_sites.len())
                    .map_err(|_| Diagnostic::unsupported(self.name, "too many Wasm call sites"))?;
                self.indirect_sites.push(WasmIndirectSite {
                    table_index: u32::MAX,
                    type_index,
                });
                let immediate = ImmediateLayout::call_immediate(base, params, false, false);
                self.emit_tail_call(Op::WasmCallIndirect, reference, site, immediate)
            }
            Operator::GlobalGet { global_index } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let (value, mutable) = self
                    .globals
                    .get(global_index as usize)
                    .copied()
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm global index out of bounds")
                    })?;
                let result = self.push()?;
                if mutable
                    || matches!(
                        value.ty(),
                        WasmType::FuncRef | WasmType::ExternRef | WasmType::I31Ref
                    )
                {
                    self.emit(Op::WasmGlobalGet, result, 0, 0, global_index)
                } else {
                    self.load_scalar(result, value)
                }
            }
            Operator::GlobalSet { global_index } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let (_, mutable) = self
                    .globals
                    .get(global_index as usize)
                    .copied()
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm global index out of bounds")
                    })?;
                if !mutable {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "write to immutable Wasm global",
                    ));
                }
                let value = self.pop()?;
                self.emit(Op::WasmGlobalSet, value, 0, 0, global_index)
            }
            Operator::MemorySize { mem, .. } => {
                if self.memories.get(mem as usize).is_none() {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm memory index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let result = self.push()?;
                self.emit(Op::WasmMemorySize, result, 0, 0, mem)
            }
            Operator::MemoryGrow { mem, .. } => {
                if self.memories.get(mem as usize).is_none() {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm memory index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let delta = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmMemoryGrow, result, delta, 0, mem)
            }
            Operator::TableGet { table } => {
                if self.tables.get(table as usize).is_none() {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm table index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let index = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmTableGet, result, index, 0, table)
            }
            Operator::TableSet { table } => {
                if self.tables.get(table as usize).is_none() {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm table index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let value = self.pop()?;
                let index = self.pop()?;
                self.emit(Op::WasmTableSet, index, value, 0, table)
            }
            Operator::TableSize { table } => {
                if self.tables.get(table as usize).is_none() {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm table index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let result = self.push()?;
                self.emit(Op::WasmTableSize, result, 0, 0, table)
            }
            Operator::TableGrow { table } => {
                if self.tables.get(table as usize).is_none() {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm table index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let delta = self.pop()?;
                let initial = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmTableGrow, result, delta, initial, table)
            }
            Operator::TableFill { table } => {
                if self.tables.get(table as usize).is_none() {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm table index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let value = self.pop()?;
                let destination = self.pop()?;
                self.emit(Op::WasmTableFill, destination, value, length, table)
            }
            Operator::TableCopy {
                dst_table,
                src_table,
            } => {
                if self.tables.get(dst_table as usize).is_none()
                    || self.tables.get(src_table as usize).is_none()
                {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm table index out of bounds",
                    ));
                }
                if dst_table > u16::MAX as u32 || src_table > u16::MAX as u32 {
                    return Err(Diagnostic::unsupported(self.name, "too many Wasm tables"));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let source = self.pop()?;
                let destination = self.pop()?;
                self.emit(
                    Op::WasmTableCopy,
                    destination,
                    source,
                    length,
                    (src_table << 16) | dst_table,
                )
            }
            Operator::TableInit { elem_index, table } => {
                if self.tables.get(table as usize).is_none()
                    || elem_index as usize >= self.element_segment_count
                {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm table.init index out of bounds",
                    ));
                }
                if table > u16::MAX as u32 {
                    return Err(Diagnostic::unsupported(self.name, "too many Wasm tables"));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let source = self.pop()?;
                let destination = self.pop()?;
                self.emit(
                    Op::WasmTableInit,
                    destination,
                    source,
                    length,
                    (table << 16) | elem_index,
                )
            }
            Operator::ElemDrop { elem_index } => {
                if elem_index as usize >= self.element_segment_count {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm elem.drop index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                self.emit(Op::WasmElemDrop, 0, 0, 0, elem_index)
            }
            Operator::RefIsNull => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let value = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmRefIsNull, result, value, 0, 0)
            }
            Operator::RefAsNonNull => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let value = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmRefAsNonNull, result, value, 0, 0)
            }
            Operator::RefEq => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let right = self.pop()?;
                let left = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmRefEq, result, left, right, 0)
            }
            Operator::RefI31 => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let value = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmRefI31, result, value, 0, 0)
            }
            Operator::I31GetS | Operator::I31GetU => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let value = self.pop()?;
                let result = self.push()?;
                let unsigned = u32::from(matches!(operator, Operator::I31GetU));
                self.emit(Op::WasmI31Get, result, value, 0, unsigned)
            }
            Operator::StructNew { struct_type_index } => {
                let field_count = match self.gc_type(struct_type_index)? {
                    WasmGcType::Struct(fields) => fields.len(),
                    _ => {
                        return Err(Diagnostic::unsupported(
                            self.name,
                            "struct.new requires a struct type",
                        ));
                    }
                };
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                for _ in 0..field_count {
                    self.pop()?;
                }
                let first_field = self.depth;
                let result = self.push()?;
                let immediate = self.gc_alloc_immediate(struct_type_index, WASM_GC_ALLOC_STRUCT)?;
                self.emit(Op::WasmGcAlloc, result, first_field, 0, immediate)
            }
            Operator::StructNewDesc { struct_type_index } => {
                let field_count = match self.gc_type(struct_type_index)? {
                    WasmGcType::Struct(fields) => fields.len(),
                    _ => {
                        return Err(Diagnostic::unsupported(
                            self.name,
                            "struct.new_desc requires a struct type",
                        ));
                    }
                };
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let descriptor = self.pop()?;
                for _ in 0..field_count {
                    self.pop()?;
                }
                let first_field = self.depth;
                let result = self.push()?;
                let immediate =
                    self.gc_alloc_immediate(struct_type_index, WASM_GC_ALLOC_STRUCT_DESC)?;
                self.emit(Op::WasmGcAlloc, result, first_field, descriptor, immediate)
            }
            Operator::StructGet {
                struct_type_index,
                field_index,
            }
            | Operator::StructGetS {
                struct_type_index,
                field_index,
            }
            | Operator::StructGetU {
                struct_type_index,
                field_index,
            } => {
                let WasmGcType::Struct(fields) = self.gc_type(struct_type_index)? else {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "struct.get requires a struct type",
                    ));
                };
                let field = fields.get(field_index as usize).ok_or_else(|| {
                    Diagnostic::unsupported(self.name, "Wasm struct field index out of bounds")
                })?;
                let is_packed_get = !matches!(operator, Operator::StructGet { .. });
                if is_packed_get != field.packed_bits.is_some() {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "struct.get packed type mismatch",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let reference = self.pop()?;
                let result = self.push()?;
                let selector = match operator {
                    Operator::StructGetS { .. } => WASM_GC_ACCESS_STRUCT_GET_S,
                    Operator::StructGetU { .. } => WASM_GC_ACCESS_STRUCT_GET_U,
                    _ => WASM_GC_ACCESS_STRUCT_GET,
                };
                let immediate = self.gc_access_immediate(selector, field_index)?;
                self.emit(Op::WasmGcAccess, result, reference, 0, immediate)
            }
            Operator::StructSet {
                struct_type_index,
                field_index,
            } => {
                let WasmGcType::Struct(fields) = self.gc_type(struct_type_index)? else {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "struct.set requires a struct type",
                    ));
                };
                let field = fields.get(field_index as usize).ok_or_else(|| {
                    Diagnostic::unsupported(self.name, "Wasm struct field index out of bounds")
                })?;
                if !field.mutable {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "struct.set requires a mutable field",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let value = self.pop()?;
                let reference = self.pop()?;
                let immediate = self.gc_access_immediate(WASM_GC_ACCESS_STRUCT_SET, field_index)?;
                self.emit(Op::WasmGcAccess, reference, value, 0, immediate)
            }
            Operator::ArrayNew { array_type_index } => {
                if !matches!(self.gc_type(array_type_index)?, WasmGcType::Array(_)) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.new requires an array type",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let initial = self.pop()?;
                let result = self.push()?;
                let immediate = self.gc_alloc_immediate(array_type_index, WASM_GC_ALLOC_ARRAY)?;
                self.emit(Op::WasmGcAlloc, result, initial, length, immediate)
            }
            Operator::ArrayNewFixed {
                array_type_index,
                array_size,
            } => {
                if !matches!(self.gc_type(array_type_index)?, WasmGcType::Array(_)) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.new_fixed requires an array type",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let count = u16::try_from(array_size).map_err(|_| {
                    Diagnostic::unsupported(self.name, "array.new_fixed has too many elements")
                })?;
                if count > self.depth.saturating_sub(self.control_base()) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm operand stack underflow",
                    ));
                }
                for _ in 0..count {
                    self.pop()?;
                }
                let first_value = self.depth;
                let count_register = self.alloc_register()?;
                self.load_i32(count_register, array_size as i32)?;
                let result = self.push()?;
                let immediate =
                    self.gc_alloc_immediate(array_type_index, WASM_GC_ALLOC_ARRAY_FIXED)?;
                self.emit(
                    Op::WasmGcAlloc,
                    result,
                    first_value,
                    count_register,
                    immediate,
                )
            }
            Operator::ArrayNewData {
                array_type_index,
                array_data_index,
            } => {
                if !matches!(self.gc_type(array_type_index)?, WasmGcType::Array(_)) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.new_data requires an array type",
                    ));
                }
                if array_data_index as usize >= self.data_segment_count {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm data segment index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let source = self.pop()?;
                let result = self.push()?;
                let immediate = self.gc_segment_alloc_immediate(
                    array_type_index,
                    array_data_index,
                    WASM_GC_ALLOC_ARRAY_DATA,
                )?;
                self.emit(Op::WasmGcAlloc, result, source, length, immediate)
            }
            Operator::ArrayNewElem {
                array_type_index,
                array_elem_index,
            } => {
                if !matches!(self.gc_type(array_type_index)?, WasmGcType::Array(_)) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.new_elem requires an array type",
                    ));
                }
                if array_elem_index as usize >= self.element_segment_count {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm element segment index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let source = self.pop()?;
                let result = self.push()?;
                let immediate = self.gc_segment_alloc_immediate(
                    array_type_index,
                    array_elem_index,
                    WASM_GC_ALLOC_ARRAY_ELEM,
                )?;
                self.emit(Op::WasmGcAlloc, result, source, length, immediate)
            }
            Operator::ArrayGet { array_type_index }
            | Operator::ArrayGetS { array_type_index }
            | Operator::ArrayGetU { array_type_index } => {
                let WasmGcType::Array(field) = self.gc_type(array_type_index)? else {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.get requires an array type",
                    ));
                };
                let selector = match operator {
                    Operator::ArrayGet { .. } if field.packed_bits.is_none() => {
                        WASM_GC_ACCESS_ARRAY_GET
                    }
                    Operator::ArrayGetS { .. } if field.packed_bits.is_some() => {
                        WASM_GC_ACCESS_ARRAY_GET_S
                    }
                    Operator::ArrayGetU { .. } if field.packed_bits.is_some() => {
                        WASM_GC_ACCESS_ARRAY_GET_U
                    }
                    _ => {
                        return Err(Diagnostic::unsupported(
                            self.name,
                            "array.get packed type mismatch",
                        ));
                    }
                };
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let index = self.pop()?;
                let reference = self.pop()?;
                let result = self.push()?;
                let immediate = self.gc_access_immediate(selector, 0)?;
                self.emit(Op::WasmGcAccess, result, reference, index, immediate)
            }
            Operator::ArraySet { array_type_index } => {
                let WasmGcType::Array(field) = self.gc_type(array_type_index)? else {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.set requires an array type",
                    ));
                };
                if !field.mutable {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.set requires a mutable array type",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let value = self.pop()?;
                let index = self.pop()?;
                let reference = self.pop()?;
                let immediate = self.gc_access_immediate(WASM_GC_ACCESS_ARRAY_SET, 0)?;
                self.emit(Op::WasmGcAccess, reference, index, value, immediate)
            }
            Operator::ArrayLen => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let reference = self.pop()?;
                let result = self.push()?;
                let immediate = self.gc_access_immediate(WASM_GC_ACCESS_ARRAY_LEN, 0)?;
                self.emit(Op::WasmGcAccess, result, reference, 0, immediate)
            }
            Operator::ArrayFill { array_type_index } => {
                let WasmGcType::Array(field) = self.gc_type(array_type_index)? else {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.fill requires an array type",
                    ));
                };
                if !field.mutable {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.fill requires a mutable array type",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let value = self.pop()?;
                let index = self.pop()?;
                let reference = self.pop()?;
                let immediate =
                    self.gc_access_register_immediate(WASM_GC_ACCESS_ARRAY_FILL, length)?;
                self.emit(Op::WasmGcAccess, reference, index, value, immediate)
            }
            Operator::ArrayCopy {
                array_type_index_dst,
                array_type_index_src,
            } => {
                let WasmGcType::Array(dst_field) = self.gc_type(array_type_index_dst)? else {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.copy requires an array type",
                    ));
                };
                if !dst_field.mutable {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.copy requires a mutable destination type",
                    ));
                }
                if !matches!(self.gc_type(array_type_index_src)?, WasmGcType::Array(_)) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.copy requires an array source type",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let source_index = self.pop()?;
                let source = self.pop()?;
                let destination_index = self.pop()?;
                let destination = self.pop()?;
                let immediate =
                    self.gc_access_pair_immediate(WASM_GC_ACCESS_ARRAY_COPY, source_index, length)?;
                self.emit(
                    Op::WasmGcAccess,
                    destination,
                    destination_index,
                    source,
                    immediate,
                )
            }
            Operator::ArrayInitData {
                array_type_index,
                array_data_index,
            } => {
                let WasmGcType::Array(field) = self.gc_type(array_type_index)? else {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.init_data requires an array type",
                    ));
                };
                if !field.mutable {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.init_data requires a mutable array type",
                    ));
                }
                if array_data_index as usize >= self.data_segment_count {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm data segment index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let source_index = self.pop()?;
                let destination_index = self.pop()?;
                let destination = self.pop()?;
                let immediate = self.gc_access_segment_immediate(
                    WASM_GC_ACCESS_ARRAY_INIT_DATA,
                    array_data_index,
                    length,
                )?;
                self.emit(
                    Op::WasmGcAccess,
                    destination,
                    destination_index,
                    source_index,
                    immediate,
                )
            }
            Operator::ArrayInitElem {
                array_type_index,
                array_elem_index,
            } => {
                let WasmGcType::Array(field) = self.gc_type(array_type_index)? else {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.init_elem requires an array type",
                    ));
                };
                if !field.mutable {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "array.init_elem requires a mutable array type",
                    ));
                }
                if array_elem_index as usize >= self.element_segment_count {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm element segment index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let source_index = self.pop()?;
                let destination_index = self.pop()?;
                let destination = self.pop()?;
                let immediate = self.gc_access_segment_immediate(
                    WASM_GC_ACCESS_ARRAY_INIT_ELEM,
                    array_elem_index,
                    length,
                )?;
                self.emit(
                    Op::WasmGcAccess,
                    destination,
                    destination_index,
                    source_index,
                    immediate,
                )
            }
            Operator::AnyConvertExtern | Operator::ExternConvertAny => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let value = self.pop()?;
                let result = self.push()?;
                let conversion = u32::from(matches!(operator, Operator::ExternConvertAny));
                self.emit(Op::WasmRefConvert, result, value, 0, conversion)
            }
            Operator::RefTestNullable { hty } | Operator::RefTestNonNull { hty } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let nullable = matches!(operator, Operator::RefTestNullable { .. });
                let type_code = reference_heap_code(hty)? | (u32::from(nullable) << 30);
                let value = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmRefTest, result, value, 0, type_code)
            }
            Operator::RefCastDescEqNonNull { hty } | Operator::RefCastDescEqNullable { hty } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let nullable = matches!(operator, Operator::RefCastDescEqNullable { .. });
                let type_code = reference_heap_code(hty)? | (u32::from(nullable) << 30);
                let descriptor = self.pop()?;
                let value = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmRefCastDescEq, result, value, descriptor, type_code)
            }
            Operator::RefCastNullable { hty } | Operator::RefCastNonNull { hty } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let nullable = matches!(operator, Operator::RefCastNullable { .. });
                let type_code = reference_heap_code(hty)? | (u32::from(nullable) << 30);
                let value = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmRefCast, result, value, 0, type_code)
            }
            Operator::StructNewDefault { struct_type_index } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let result = self.push()?;
                let immediate =
                    self.gc_alloc_immediate(struct_type_index, WASM_GC_ALLOC_STRUCT_DEFAULT)?;
                self.emit(Op::WasmGcAlloc, result, 0, 0, immediate)
            }
            Operator::StructNewDefaultDesc { struct_type_index } => {
                if !matches!(self.gc_type(struct_type_index)?, WasmGcType::Struct(_)) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "struct.new_default_desc requires a struct type",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let descriptor = self.pop()?;
                let result = self.push()?;
                let immediate =
                    self.gc_alloc_immediate(struct_type_index, WASM_GC_ALLOC_STRUCT_DEFAULT_DESC)?;
                self.emit(Op::WasmGcAlloc, result, descriptor, 0, immediate)
            }
            Operator::RefGetDesc { type_index } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let value = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmRefGetDesc, result, value, 0, type_index)
            }
            Operator::ArrayNewDefault { array_type_index } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let result = self.push()?;
                let immediate =
                    self.gc_alloc_immediate(array_type_index, WASM_GC_ALLOC_ARRAY_DEFAULT)?;
                self.emit(Op::WasmGcAlloc, result, 0, length, immediate)
            }
            Operator::MemoryFill { mem } => {
                if self.memories.get(mem as usize).is_none() {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm memory index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let value = self.pop()?;
                let destination = self.pop()?;
                self.emit(Op::WasmMemoryFill, destination, value, length, mem)
            }
            Operator::MemoryCopy { dst_mem, src_mem } => {
                if self.memories.get(dst_mem as usize).is_none()
                    || self.memories.get(src_mem as usize).is_none()
                {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm memory index out of bounds",
                    ));
                }
                if dst_mem > u16::MAX as u32 || src_mem > u16::MAX as u32 {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "too many memories for memory.copy",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let source = self.pop()?;
                let destination = self.pop()?;
                let selector = (src_mem << 16) | dst_mem;
                self.emit(Op::WasmMemoryCopy, destination, source, length, selector)
            }
            Operator::MemoryInit { data_index, mem } => {
                if self.memories.get(mem as usize).is_none()
                    || data_index as usize >= self.data_segment_count
                {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm memory.init index out of bounds",
                    ));
                }
                if mem > u16::MAX as u32 {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "too many memories for memory.init",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let length = self.pop()?;
                let source = self.pop()?;
                let destination = self.pop()?;
                let selector = (mem << 16) | data_index;
                self.emit(Op::WasmMemoryInit, destination, source, length, selector)
            }
            Operator::DataDrop { data_index } => {
                if data_index as usize >= self.data_segment_count {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm data.drop index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                self.emit(Op::WasmDataDrop, 0, 0, 0, data_index)
            }
            Operator::RefFunc { function_index } => {
                if self.signatures.get(function_index as usize).is_none() {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm ref.func function index out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let result = self.push()?;
                self.emit(Op::WasmRefFunc, result, 0, 0, function_index)
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
