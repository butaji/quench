/// The one table of the Wasm GC value opcodes the value lane runs, each with its step (`GcStep::of`)
/// or `None` for a call target (`Call`, which the step does not own). `wasm_gc_table!` hands the
/// entries to a projector: `wasm_gc_step_of` makes `GcStep::of`'s match, `wasm_gc_op_of` makes
/// `GcStep::op`'s reverse match, `wasm_gc_names` the bare names the lane's handler table and
/// `wasm_gc_value_ops!` apply their macro to. The list is written
/// here once; nothing else names these opcodes as a set.
#[macro_export]
#[doc(hidden)]
macro_rules! wasm_gc_table {
    ($projector:ident $extra:ident) => {
        $crate::$projector! {
            $extra ;
            WasmStructGet => Some(GcStep::StructGet { signed: None }),
            WasmStructGetS => Some(GcStep::StructGet { signed: Some(true) }),
            WasmStructGetU => Some(GcStep::StructGet { signed: Some(false) }),
            WasmStructSet => Some(GcStep::StructSet),
            WasmArrayGet => Some(GcStep::ArrayGet { signed: None }),
            WasmArrayGetS => Some(GcStep::ArrayGet { signed: Some(true) }),
            WasmArrayGetU => Some(GcStep::ArrayGet { signed: Some(false) }),
            WasmArraySet => Some(GcStep::ArraySet),
            WasmStructGetRaw => Some(GcStep::StructGet { signed: None }),
            WasmStructSetRaw => Some(GcStep::StructSet),
            WasmArrayGetRaw => Some(GcStep::ArrayGet { signed: None }),
            WasmArraySetRaw => Some(GcStep::ArraySet),
            WasmArrayLen => Some(GcStep::ArrayLen),
            WasmRefIsNull => Some(GcStep::IsNull),
            WasmRefAsNonNull => Some(GcStep::AsNonNull),
            WasmRefEq => Some(GcStep::Equal),
            WasmRefTest => Some(GcStep::Test { cast: false, descriptor: false }),
            WasmRefCast => Some(GcStep::Test { cast: true, descriptor: false }),
            WasmI31 => Some(GcStep::I31),
            WasmExternalConversion => Some(GcStep::ExternConversion),
            WasmArrayNewData => Some(GcStep::ArraySegment { kind: ArraySegmentKind::Data, init: false }),
            WasmArrayNewElem => Some(GcStep::ArraySegment { kind: ArraySegmentKind::Element, init: false }),
            WasmArrayInitData => Some(GcStep::ArraySegment { kind: ArraySegmentKind::Data, init: true }),
            WasmArrayInitElem => Some(GcStep::ArraySegment { kind: ArraySegmentKind::Element, init: true }),
            WasmArrayFill => Some(GcStep::ArrayFill),
            WasmArrayCopy => Some(GcStep::ArrayCopy),
            WasmArrayNew => Some(GcStep::ArrayNew { construction: ArrayConstruction::Repeated }),
            WasmArrayNewDefault => Some(GcStep::ArrayNew { construction: ArrayConstruction::Default }),
            WasmArrayNewFixed => Some(GcStep::ArrayNew { construction: ArrayConstruction::Fixed }),
            WasmStructNew => Some(GcStep::StructNew { default: false, descriptor: false }),
            WasmStructNewDefault => Some(GcStep::StructNew { default: true, descriptor: false }),
            WasmStructNewDesc => Some(GcStep::StructNew { default: false, descriptor: true }),
            WasmStructNewDefaultDesc => Some(GcStep::StructNew { default: true, descriptor: true }),
            WasmRefGetDesc => Some(GcStep::GetDescriptor),
            WasmDescriptorTest => Some(GcStep::Test { cast: false, descriptor: true }),
            WasmDescriptorCast => Some(GcStep::Test { cast: true, descriptor: true }),
            WasmIndirectTarget => None,
            WasmRefFunc => None,
        }
    };
}

/// Projects `wasm_gc_table!`'s entries to the match of `GcStep::of`, over the opcode `$op`.
#[macro_export]
#[doc(hidden)]
macro_rules! wasm_gc_step_of {
    ($op:ident ; $($name:ident => $step:expr,)*) => {
        match $op {
            $(Op::$name => $step,)*
            _ => None,
        }
    };
}

/// Projects `wasm_gc_table!`'s entries to the reverse map `GcStep::op`: the opcode whose entry is
/// the step `$step`. Each value step has one entry, so the first match is the one.
#[macro_export]
#[doc(hidden)]
macro_rules! wasm_gc_op_of {
    ($step:ident ; $($name:ident => $entry:expr,)*) => {
        {
            $(if $entry == $step { return Op::$name; })*
            unreachable!("every GC value step has an opcode")
        }
    };
}

/// Projects `wasm_gc_table!`'s entries to the bare opcode names, applying `$then!` to them.
#[macro_export]
#[doc(hidden)]
macro_rules! wasm_gc_names {
    ($then:ident ; $($name:ident => $group:expr,)*) => {
        $then!($($name)*)
    };
}

/// Applies `$then!` to the names of every Wasm GC value opcode: the one list (`wasm_gc_table!`)
/// that `GcOp::of` and the lane's handlers both derive from.
#[macro_export]
#[doc(hidden)]
macro_rules! wasm_gc_value_ops {
    ($then:ident) => {
        $crate::wasm_gc_table!(wasm_gc_names $then)
    };
}
