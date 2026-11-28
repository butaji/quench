//! The method one lane body's code makes, from facts its plan or its image established. Kept
//! apart from the JIT plan so an image installer links no lowering or machine IR.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::lane_compile::LaneDependencies;

/// What a lane body's plan and lowering established: everything `finish_lane_method` holds but
/// its code. An image body linked at its run address has the same facts without a lowering.
pub(crate) struct LaneMethodFacts {
    pub(crate) unit: lane_unit::Unit,
    pub(crate) stable: lane_region::StableFacts,
    pub(crate) fields: Vec<Option<Vec<NativeFieldLoad>>>,
    pub(crate) direct: Box<[DirectArm]>,
    pub(crate) closed: bool,
    pub(crate) function: u32,
    pub(crate) invalidation: quench_runtime::code_dependencies::Invalidation,
    pub(crate) fault_poll: bool,
    pub(crate) exits: ExitTable,
    pub(crate) summary: Option<lane_region::TypeSummary>,
    pub(crate) clif: Option<String>,
    /// Whether the code makes proven calls (`LaneMethod::proven_calls`): an image body's, whose
    /// calls name no `direct` callee but still bind shadow frames.
    pub(crate) proven_calls: bool,
}

/// The method one lane body's code makes, from what its plan or its image established. Shared by
/// a freshly compiled body and by an image body, so installing an image never constructs the
/// lowering or its machine IR.
pub(crate) fn finish_lane_method(code: LaneCode, scalar: Option<usize>, proven: Option<(usize, Box<[quench_runtime::typed_ir::Type]>)>, facts: LaneMethodFacts) -> Option<LaneMethod> {
    let LaneMethodFacts { unit, stable, fields, direct, closed, function, invalidation, fault_poll, exits, summary, clif, proven_calls } = facts;
    let (dependencies, recovery_space) = (LaneDependencies::of(fields.iter().flatten().flatten().copied(), &direct), super::dispatch_fast::LaneRecoverySpace::inlined(&unit.frames));
    let (scratch_width, caller_registers) = (usize::from(unit.registers - unit.caller_registers), unit.caller_registers);
    let names_shapes = !fields.is_empty() || !direct.is_empty();
    let (pointer, length) = code.span();
    if let (Some(directory), Some(clif)) = (native_dump_directory(), clif.as_deref()) {
        // SAFETY: the code is `length` readable bytes while `code` lives (or for the process, where placed).
        let bytes = unsafe { std::slice::from_raw_parts(pointer, length) };
        dump_native_bytes(directory, DumpSite::Body { function, scalar }, bytes, Some(pointer), clif);
    }
    // SAFETY: the lane region lowers a body with the lane body ABI (`body_signature_in_lane`).
    let entry = unsafe { std::mem::transmute::<*mut u8, BodyEntry>(pointer) };
    let scalar = scalar.map(|at| pointer.addr() + at);
    let proven = proven.map(|(at, params)| (pointer.addr() + at, params));
    Some(LaneMethod {
        dependencies, stable, invalidation, _code: code.owned(), entry, scalar, proven, rows: None, recovery_space,
        fault_poll: fault_poll || direct.iter().any(|callee| callee.body.body.as_ref().is_some_and(|body| body.fault_poll)),
        // A body that names a callee's body retires with it (`DirectBody::method`).
        names_shapes, exits, inlined: unit.frames, scratch_width, caller_registers, frame_reach: 0,
        direct, proven_calls, closed, framed: false, _records: Vec::new(), constructors: Box::default(),
        realm_bound: false, summary, withdrawn: Default::default(),
    })
}


/// Where a lane body's code lives: a code heap block it owns, or text the system linker placed in the executable
/// (`aot_image::object`), linked in place at the image's init edge and alive for the process.
pub(crate) enum LaneCode {
    Owned(CodeMemory),
    Placed { pointer: *mut u8, length: usize },
}

impl LaneCode {
    /// The code's first byte and length.
    fn span(&self) -> (*mut u8, usize) {
        match self {
            Self::Owned(memory) => (memory.pointer(), memory.length()),
            Self::Placed { pointer, length } => (*pointer, *length),
        }
    }

    /// The block the method keeps alive, none where the executable holds the code.
    fn owned(self) -> Option<CodeMemory> {
        match self {
            Self::Owned(memory) => Some(memory),
            Self::Placed { .. } => None,
        }
    }
}
