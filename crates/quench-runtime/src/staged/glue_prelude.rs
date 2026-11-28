//! Every extension trait and glue free function of the R8 split, in one import. Call sites that
//! name a glued method write `use crate::staged::glue_prelude::*;` (engine: `quench_runtime::
//! staged::glue_prelude::*`). Each name is re-exported from the module that defines it, so this
//! file is the only list to extend when a glue trait is added.

pub use super::operation::{OperationOperationExt, OperationOperationRecordsExt};
pub use super::names::OperationNamesExt;
pub use super::generic::OperationGenericExt;
pub use super::compare::{ComparisonCompareExt, OperationCompareExt};
pub use super::element_rows::OperationElementRowsExt;
pub use super::forms::{CallRecordFormsExt, InvocationFormsExt};
pub use super::suspend::OperationSuspendExt;
pub use super::wasm_state::{OperationWasmStateExt, WasmStateOperationWasmStateExt};
pub use super::wasm_access::{OperationWasmAccessExt, WasmAccessOperationWasmAccessExt};
pub use super::wasm_jump::{BranchTableWasmJumpExt, WasmJumpWasmJumpExt};
pub use super::superinstructions::{BinaryPartSuperinstructionsExt, ConstArrayPairSuperinstructionsExt};
pub use super::wasm_operation::{FormWasmOperationExt, WasmOperationWasmOperationExt};
pub use super::protocol_wasm::{ProtocolProtocolWasmExt, WasmHostFieldsProtocolWasmExt, WasmHostStepProtocolWasmExt};
pub use super::allocation::AllocationAllocationExt;
pub use super::wasm_memory_bytes::{MemoryLoadWasmMemoryBytesExt, MemoryStoreWasmMemoryBytesExt};
pub use super::wasm_float::{OperationWasmFloatExt, WasmFloatOperationWasmFloatExt};
pub use super::wasm_simd::{OperationWasmSimdExt, SimdWasmSimdExt, VectorOperationWasmSimdExt};
pub use super::wasm_instance::InstanceWasmInstanceExt;
pub use super::wasm_call::CallWasmCallExt;
pub use super::wasm_memory::{AccessWasmMemoryExt, MemoryWasmMemoryExt};
pub use super::division::OperationDivisionExt;
pub use super::c_twins::{NumberOperationCTwinsExt, OperationCTwinsExt};
pub use crate::wasm::WasmValueScalarExt;
pub use crate::wasm::memory::MemoryLoadMemoryOpsExt;
pub use crate::wasm::simd::SimdOperatorSimdExt;
pub use crate::heap::{
    RecordBlockRecordsExt, TypedArrayKindTypedArrayKindExt, typed_array_kind_integer_signed, typed_array_kind_is_bigint,
    typed_array_kind_is_float, typed_array_kind_is_waitable, typed_array_kind_name, typed_array_kind_native,
};
