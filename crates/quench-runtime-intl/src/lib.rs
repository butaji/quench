#![allow(dead_code, private_interfaces)]

pub mod dispatch;
pub mod intl_collator;
pub mod intl_datetime;
pub mod intl_datetime_parts;
pub mod intl_display_names;
pub mod intl_duration_format;
pub mod intl_list_format;
pub mod intl_namespace;
pub mod intl_number;
pub mod intl_plural_rules;
pub mod intl_relative;
pub mod intl_segmenter;
pub mod temporal;
pub mod temporal_date;
pub mod temporal_date_arithmetic;
pub mod temporal_date_difference;
pub mod temporal_date_parse;
pub mod temporal_date_projection;
pub mod temporal_date_time;
pub mod temporal_date_time_difference;
pub mod temporal_instant;
pub mod temporal_instant_difference;
pub mod temporal_instant_format;
pub mod temporal_instant_round;
pub mod temporal_plain_date_time_conversion;
pub mod temporal_plain_time;
pub mod temporal_zoned_date_time;

/// What every layer module sees: the VM core, the layer's modules and its extension traits.
#[allow(unused_imports)]
pub(crate) mod intl_prelude {
    pub use quench_runtime::vm::*;
    pub use quench_runtime::vm::intl_core::Collation;
    // The names the VM core's modules import (`quench-runtime/src/vm.rs`), which its submodules saw.
    pub use quench_runtime::vm::engine_types::InstanceRecord;
    pub use quench_runtime::vm::wasm_exception::{js_tag_cell, js_tag_type};
    pub use quench_runtime::Value;
    pub use quench_runtime::bytecode::{Atom, AtomTable, Constant, DispatchClass, FieldBase, Instr, LEXICAL_THIS_BINDING,
    NEW_TARGET_BINDING, Op, Operand, Register, ResidualProgram, WideInstruction};
    pub use quench_runtime::staged::add_plan::{AddSite, Transition, concrete_add_property, concrete_set_function_name};
    pub use quench_runtime::library_surface::CellKind;
    pub use quench_runtime::library_surface::{
    CallSiteLocation, CallSiteName, CallSiteRecord, Cell, CollectionStorage, FunctionKind, FunctionRecord, GcTraceKind, IteratorRecord, Heap, IteratorConsumer,
    IteratorHelper, IteratorKind, Native, Object, ProtoLink, ProxyKind, RootId, StackData, TypedArrayKind,
    WeakHandle,
};
    pub use quench_runtime::host::{CapabilityId, Host, HostContext};
    pub use quench_runtime::profile::{CallSource, Profile, TerminalCall};
    pub use quench_runtime::value::number_to_u32;
    pub use quench_runtime::vm::activation::{CallCompletion, CallContext, ConstructionTarget, TrapAccess, TrapCheck};
    pub use quench_runtime::vm::atomics::Test262AgentState;
    pub use rustc_hash::{FxHashMap, FxHashSet};
    pub use std::cell::OnceCell;
    pub use std::hash::{Hash, Hasher};
    pub use std::rc::Rc;
    pub use quench_runtime::units::MAX_SAFE_INTEGER;
    pub use quench_runtime::bytecode::MAX_ARRAY_LENGTH;
    pub use quench_runtime::vm::pending_job::PendingJob;
    pub use quench_runtime::vm::atom_set::{AtomSet, IndexAtoms};
    pub use quench_runtime::vm::call_printer::{freeze_call_sites, freeze_source_call_sites};
    pub use quench_runtime::vm::engine_types::{copy_arguments, fill_window as fill_registers};
    pub use quench_runtime::vm::group_by::GroupByKind;
    pub use quench_runtime::vm::property_definition::PropertyDefinitionKind;
    pub use quench_runtime::vm::call_arguments::CallArguments;
    pub use quench_runtime::vm::object::AtomClass;
    pub use quench_runtime::vm::json::json_literal_length;
    pub use quench_runtime::vm::object_static::array_index;
    pub use quench_runtime::vm::program_store::ProgramId;
    pub use quench_runtime::vm::regexp::RegExpFlags;
    pub use quench_runtime::vm::program_store::{ModuleImport, ProgramStore};
    pub use quench_runtime::vm::promise::PromiseRuntime;
    pub use quench_runtime::vm::property_key::PropertyKey;
    pub use quench_runtime::vm::root_bindings::RootBinding;
    pub use quench_runtime::vm::well_known_symbols::{WellKnownSymbol, WellKnownSymbols};
    pub use quench_runtime::vm::value_serializer::{ReadHooks, WriteHooks};
    pub use quench_runtime::vm::wtf16::JsString;
    pub use quench_runtime::staged::field_cache_layout::{self, FieldCacheWords};
    pub use quench_runtime::staged::layout_offsets::NATIVE_FIELD_CACHE_LAYOUT;
    pub use quench_runtime::vm::engine_types::AHEAD_BUILD_ID_PREFIX;
    pub use crate::dispatch::IntlTemporalDispatch;
    pub use crate::intl_collator;
    pub use crate::intl_datetime;
    pub use crate::intl_datetime_parts;
    pub use crate::intl_display_names;
    pub use crate::intl_duration_format;
    pub use crate::intl_list_format;
    pub use crate::intl_namespace;
    pub use crate::intl_number;
    pub use crate::intl_plural_rules;
    pub use crate::intl_relative;
    pub use crate::intl_segmenter;
    pub use crate::temporal;
    pub use crate::temporal_date;
    pub use crate::temporal_date_arithmetic;
    pub use crate::temporal_date_difference;
    pub use crate::temporal_date_parse;
    pub use crate::temporal_date_projection;
    pub use crate::temporal_date_time;
    pub use crate::temporal_date_time_difference;
    pub use crate::temporal_instant;
    pub use crate::temporal_instant_difference;
    pub use crate::temporal_instant_format;
    pub use crate::temporal_instant_round;
    pub use crate::temporal_plain_date_time_conversion;
    pub use crate::temporal_plain_time;
    pub use crate::temporal_zoned_date_time;
    pub use crate::intl_collator::IntlCollatorVm;
    pub use crate::intl_datetime::IntlDatetimeVm;
    pub use crate::intl_display_names::IntlDisplayNamesVm;
    pub use crate::intl_duration_format::IntlDurationFormatVm;
    pub use crate::intl_list_format::IntlListFormatVm;
    pub use crate::intl_namespace::IntlNamespaceVm;
    pub use crate::intl_number::IntlNumberVm;
    pub use crate::intl_plural_rules::IntlPluralRulesVm;
    pub use crate::intl_relative::IntlRelativeVm;
    pub use crate::intl_segmenter::IntlSegmenterVm;
    pub use crate::temporal::TemporalVm;
    pub use crate::temporal_date::TemporalDateVm;
    pub use crate::temporal_date_arithmetic::TemporalDateArithmeticVm;
    pub use crate::temporal_date_difference::TemporalDateDifferenceVm;
    pub use crate::temporal_date_projection::TemporalDateProjectionVm;
    pub use crate::temporal_date_time::TemporalDateTimeVm;
    pub use crate::temporal_date_time_difference::TemporalDateTimeDifferenceVm;
    pub use crate::temporal_instant::TemporalInstantVm;
    pub use crate::temporal_plain_time::TemporalPlainTimeVm;
    pub use crate::temporal_zoned_date_time::TemporalZonedDateTimeVm;
}
