use super::activation::ContinuationId;
use super::module::{ModuleEvaluationStack, ModuleOutcome, ModulePhase, ModuleRecord};
use super::*;
use std::collections::VecDeque;

const PROMISE_CAPABILITY_RESOLVE: &str = "\0rqj:promise-capability-resolve";
const PROMISE_CAPABILITY_REJECT: &str = "\0rqj:promise-capability-reject";
use crate::ModuleSource;
use rustc_hash::FxHashSet;

fn module_cache_key(name: &str, module_type: &str) -> String {
    let name = crate::module_identity::normalize(std::path::Path::new(name));
    format!("{}:{module_type}", name.display())
}

fn self_import_referrer(referrer: &str, resolved: &str) -> bool {
    crate::module_identity::normalize(std::path::Path::new(referrer))
        == crate::module_identity::normalize(std::path::Path::new(resolved))
}

#[derive(Clone)]
enum StaticModuleValue {
    Constant {
        module: String,
        export: String,
        value: Constant,
    },
    Cached(Value),
    ModuleSource(Value),
    Binding {
        program: ProgramId,
        slot: u16,
        value: Value,
    },
    Namespace {
        name: String,
        exports: Vec<(String, StaticModuleValue)>,
    },
}

type ActiveModuleExports = FxHashMap<std::path::PathBuf, Vec<(String, StaticModuleValue)>>;

type StaticModuleNodes = FxHashMap<std::path::PathBuf, StaticModuleNode>;
type StaticModuleResolveSet = FxHashSet<(std::path::PathBuf, String)>;

#[derive(Clone)]
enum StaticModuleNode {
    Direct(Vec<(String, StaticModuleValue)>),
    Planned {
        locals: Vec<(String, StaticModuleValue)>,
        reexports: Vec<crate::compile::StaticModuleReexport>,
    },
}

enum StaticModuleResolution {
    Binding(StaticModuleValue),
    Missing,
    Ambiguous,
    Unsupported,
}

enum StaticModuleGraph {
    Linked {
        name: String,
        exports: Vec<(String, StaticModuleValue)>,
        incomplete: bool,
    },
    Unsupported,
    LinkError,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StaticModuleAdd {
    Added,
    Missing,
    Conflict,
}

#[derive(Default)]
struct StaticModuleLinks {
    explicit: FxHashMap<String, StaticModuleValue>,
    stars: FxHashMap<String, StaticModuleValue>,
    ambiguous: FxHashSet<String>,
}

impl StaticModuleLinks {
    fn add_local(&mut self, name: String, value: StaticModuleValue) -> StaticModuleAdd {
        self.insert_explicit(name, value)
    }

    fn add_locals(&mut self, locals: Vec<(String, StaticModuleValue)>) -> StaticModuleAdd {
        for (name, value) in locals {
            if self.add_local(name, value) != StaticModuleAdd::Added {
                return StaticModuleAdd::Conflict;
            }
        }
        StaticModuleAdd::Added
    }

    fn add(
        &mut self,
        reexport: crate::compile::StaticModuleReexport,
        dependency: String,
        exports: Vec<(String, StaticModuleValue)>,
    ) -> StaticModuleAdd {
        match reexport {
            crate::compile::StaticModuleReexport::Named {
                imported, exported, ..
            } => {
                let Some(value) = exports
                    .into_iter()
                    .find_map(|(name, value)| (name == imported).then_some(value))
                else {
                    return StaticModuleAdd::Missing;
                };
                self.insert_explicit(exported, value)
            }
            crate::compile::StaticModuleReexport::Namespace { exported, .. } => self
                .insert_explicit(
                    exported,
                    StaticModuleValue::Namespace {
                        name: dependency,
                        exports,
                    },
                ),
            crate::compile::StaticModuleReexport::Star { .. } => {
                for (name, value) in exports {
                    if name == "default"
                        || self.explicit.contains_key(&name)
                        || self.ambiguous.contains(&name)
                    {
                        continue;
                    }
                    match self.stars.get(&name) {
                        Some(existing) if same_static_module_binding(existing, &value) => {}
                        Some(_) => {
                            self.stars.remove(&name);
                            self.ambiguous.insert(name);
                        }
                        None => {
                            self.stars.insert(name, value);
                        }
                    }
                }
                StaticModuleAdd::Added
            }
        }
    }

    fn insert_explicit(&mut self, name: String, value: StaticModuleValue) -> StaticModuleAdd {
        if self.explicit.insert(name, value).is_some() {
            StaticModuleAdd::Conflict
        } else {
            StaticModuleAdd::Added
        }
    }

    fn partial_exports(&self) -> Vec<(String, StaticModuleValue)> {
        let mut exports = self.explicit.clone();
        exports.extend(self.stars.clone());
        let mut exports = exports.into_iter().collect::<Vec<_>>();
        exports.sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));
        exports
    }

    fn finish(self, name: String, incomplete: bool) -> StaticModuleGraph {
        let exports = self.partial_exports();
        StaticModuleGraph::Linked {
            name,
            exports,
            incomplete,
        }
    }
}

fn same_static_module_binding(left: &StaticModuleValue, right: &StaticModuleValue) -> bool {
    match (left, right) {
        (
            StaticModuleValue::Binding {
                program: left_program,
                slot: left_slot,
                ..
            },
            StaticModuleValue::Binding {
                program: right_program,
                slot: right_slot,
                ..
            },
        ) => left_program == right_program && left_slot == right_slot,
        (
            StaticModuleValue::Constant {
                module: left_module,
                export: left_export,
                ..
            },
            StaticModuleValue::Constant {
                module: right_module,
                export: right_export,
                ..
            },
        ) => {
            left_export == right_export
                && crate::module_identity::normalize(std::path::Path::new(left_module))
                    == crate::module_identity::normalize(std::path::Path::new(right_module))
        }
        (
            StaticModuleValue::Namespace { name: left, .. },
            StaticModuleValue::Namespace { name: right, .. },
        ) => {
            crate::module_identity::normalize(std::path::Path::new(left))
                == crate::module_identity::normalize(std::path::Path::new(right))
        }
        (StaticModuleValue::Cached(left), StaticModuleValue::Cached(right)) => left == right,
        (StaticModuleValue::ModuleSource(left), StaticModuleValue::ModuleSource(right)) => {
            left == right
        }
        _ => false,
    }
}

fn native_length(kind: Native) -> Option<f64> {
    if let Some(length) = super::date::date_native_length(kind) {
        return Some(length);
    }
    if let Some(length) = super::error::error_native_length(kind) {
        return Some(length);
    }
    if let Some(length) = super::finalization::finalization_native_length(kind) {
        return Some(length);
    }
    if let Some(length) = super::finalization::disposal_native_length(kind) {
        return Some(length);
    }
    Some(match kind {
        Native::FunctionPrototype | Native::FunctionToString | Native::FunctionCaller => 0.0,
        Native::FunctionPrototypeHasInstance => 1.0,
        Native::FunctionCall | Native::FunctionBind => 1.0,
        Native::FunctionApply => 2.0,
        Native::AbstractModuleSource => 0.0,
        Native::AbstractModuleSourceToStringTag => 0.0,
        Native::AggregateError => 2.0,
        Native::BigInt => 1.0,
        Native::Boolean => 1.0,
        Native::BooleanToString | Native::BooleanValueOf => 0.0,
        Native::DataView => 1.0,
        Native::DataViewGetBigInt64
        | Native::DataViewGetBigUint64
        | Native::DataViewGetFloat16
        | Native::DataViewGetFloat32
        | Native::DataViewGetFloat64
        | Native::DataViewGetInt8
        | Native::DataViewGetInt16
        | Native::DataViewGetInt32
        | Native::DataViewGetUint8
        | Native::DataViewGetUint16
        | Native::DataViewGetUint32 => 1.0,
        Native::DataViewSetBigInt64
        | Native::DataViewSetBigUint64
        | Native::DataViewSetFloat16
        | Native::DataViewSetFloat32
        | Native::DataViewSetFloat64
        | Native::DataViewSetInt8
        | Native::DataViewSetInt16
        | Native::DataViewSetInt32
        | Native::DataViewSetUint8
        | Native::DataViewSetUint16
        | Native::DataViewSetUint32 => 2.0,
        Native::DataViewBufferGetter
        | Native::DataViewByteLengthGetter
        | Native::DataViewByteOffsetGetter => 0.0,
        Native::BigIntAsIntN | Native::BigIntAsUintN => 2.0,
        Native::BigIntValueOf | Native::BigIntToString => 0.0,
        Native::Number => 1.0,
        Native::NumberValueOf | Native::NumberToLocaleString => 0.0,
        Native::NumberString
        | Native::NumberIsNaN
        | Native::NumberIsFinite
        | Native::NumberIsInteger
        | Native::NumberIsSafeInteger
        | Native::NumberParseFloat
        | Native::NumberFixed
        | Native::NumberExponential
        | Native::NumberPrecision => 1.0,
        Native::ParseInt => 2.0,
        Native::SuppressedError => 3.0,
        Native::Error
        | Native::EvalError
        | Native::RangeError
        | Native::ReferenceError
        | Native::SyntaxError
        | Native::TypeError
        | Native::URIError => 1.0,
        Native::Promise => 1.0,
        Native::PromiseSpeciesGetter => 0.0,
        Native::PromiseResolve
        | Native::PromiseReject
        | Native::PromiseCatch
        | Native::PromiseFinally => 1.0,
        Native::PromiseThen => 2.0,
        Native::PromiseAll
        | Native::PromiseAllKeyed
        | Native::PromiseRace
        | Native::PromiseAllSettled
        | Native::PromiseAllSettledKeyed
        | Native::PromiseAny => 1.0,
        Native::PromiseFinallyHandler => 1.0,
        Native::PromiseFinallyContinuationHandler => 0.0,
        Native::PromiseAggregateJob => 1.0,
        Native::PromiseWithResolvers => 0.0,
        Native::PromiseCapabilityExecutor => 2.0,
        Native::Object => 1.0,
        Native::String => 1.0,
        Native::StringToLocaleLowerCase | Native::StringToLocaleUpperCase => 0.0,
        Native::StringLocaleCompare => 1.0,
        Native::StringFromCharCode | Native::StringFromCodePoint => 1.0,
        Native::RegExp => 2.0,
        Native::ObjectPrototypeToLocaleString | Native::ObjectPrototypeValueOf => 0.0,
        Native::ObjectPrototypeToString => 0.0,
        Native::ObjectPrototypeDefineGetter | Native::ObjectPrototypeDefineSetter => 2.0,
        Native::ObjectPrototypeProtoGetter => 0.0,
        Native::ObjectPrototypeProtoSetter => 1.0,
        Native::ObjectPrototypeLookupGetter
        | Native::ObjectPrototypeLookupSetter
        | Native::ObjectPrototypeHasOwnProperty
        | Native::ObjectPrototypePropertyIsEnumerable
        | Native::ObjectPrototypeIsPrototypeOf => 1.0,
        Native::Map | Native::Set | Native::MapSizeGetter | Native::SetSizeGetter => 0.0,
        Native::MapGet | Native::MapHas | Native::MapDelete | Native::MapForEach => 1.0,
        Native::MapSet => 2.0,
        Native::MapClear | Native::MapKeys | Native::MapValues | Native::MapEntries => 0.0,
        Native::MapGroupBy => 2.0,
        Native::MapGetOrInsert | Native::MapGetOrInsertComputed => 2.0,
        Native::MathMax
        | Native::MathMin
        | Native::MathPow
        | Native::MathAtan2
        | Native::MathHypot
        | Native::MathImul => 2.0,
        Native::MathRandom => 0.0,
        Native::MathAbs
        | Native::MathAcos
        | Native::MathAcosh
        | Native::MathAsin
        | Native::MathAsinh
        | Native::MathAtan
        | Native::MathAtanh
        | Native::MathCbrt
        | Native::MathCeil
        | Native::MathClz32
        | Native::MathCos
        | Native::MathCosh
        | Native::MathExp
        | Native::MathExpm1
        | Native::MathF16Round
        | Native::MathFloor
        | Native::MathFround
        | Native::MathLog
        | Native::MathLog10
        | Native::MathLog1p
        | Native::MathLog2
        | Native::MathRound
        | Native::MathSign
        | Native::MathSin
        | Native::MathSinh
        | Native::MathSqrt
        | Native::MathSumPrecise
        | Native::MathTan
        | Native::MathTanh
        | Native::MathTrunc => 1.0,
        Native::SetAdd | Native::SetHas | Native::SetDelete | Native::SetForEach => 1.0,
        Native::SetClear | Native::SetKeys | Native::SetValues | Native::SetEntries => 0.0,
        Native::ToString => 1.0,
        Native::ArrayFrom | Native::ArrayFromAsync | Native::ArrayIsArray => 1.0,
        Native::ArrayBuffer | Native::ArrayBufferIsView | Native::DetachArrayBuffer => 1.0,
        Native::Iterator => 0.0,
        Native::IteratorFrom => 1.0,
        Native::IteratorConcat => 0.0,
        Native::IteratorZip | Native::IteratorZipKeyed => 1.0,
        Native::IteratorMap
        | Native::IteratorFilter
        | Native::IteratorTake
        | Native::IteratorDrop
        | Native::IteratorFlatMap
        | Native::IteratorReduce
        | Native::IteratorForEach
        | Native::IteratorEvery
        | Native::IteratorFind
        | Native::IteratorSome => 1.0,
        Native::IteratorToArray => 0.0,
        Native::IteratorDispose | Native::IteratorProtocolNext | Native::IteratorProtocolReturn => {
            0.0
        }
        Native::IteratorHelperNext | Native::IteratorHelperReturn => 0.0,
        Native::IteratorPrototypeConstructorGetter | Native::IteratorPrototypeToStringTagGetter => {
            0.0
        }
        Native::IteratorPrototypeConstructorSetter | Native::IteratorPrototypeToStringTagSetter => {
            1.0
        }
        Native::ArrayIteratorNext
        | Native::IteratorNext
        | Native::IteratorSelf
        | Native::AsyncIteratorSelf
        | Native::AsyncIteratorDispose => 0.0,
        Native::GeneratorNext
        | Native::GeneratorReturn
        | Native::GeneratorThrow
        | Native::AsyncGeneratorNext
        | Native::AsyncGeneratorReturn
        | Native::AsyncGeneratorThrow => 1.0,
        Native::AtomicsAdd
        | Native::AtomicsAnd
        | Native::AtomicsOr
        | Native::AtomicsSub
        | Native::AtomicsXor
        | Native::AtomicsExchange
        | Native::AtomicsNotify => 3.0,
        Native::AtomicsCompareExchange => 4.0,
        Native::AtomicsIsLockFree => 1.0,
        Native::AtomicsLoad => 2.0,
        Native::AtomicsStore => 3.0,
        Native::AtomicsWait | Native::AtomicsWaitAsync => 4.0,
        Native::AtomicsPause => 0.0,
        Native::AsyncDisposableStack
        | Native::AsyncDisposableStackMove
        | Native::AsyncDisposableStackDisposeAsync
        | Native::AsyncDisposableStackDisposed => 0.0,
        Native::AsyncDisposableStackUse | Native::AsyncDisposableStackDefer => 1.0,
        Native::AsyncDisposableStackAdopt => 2.0,
        Native::AsyncFunction | Native::GeneratorFunction | Native::AsyncGeneratorFunction => 1.0,
        Native::ArrayBufferSlice | Native::ArrayBufferSliceToImmutable => 2.0,
        Native::ArrayBufferResize => 1.0,
        Native::ArrayBufferTransfer
        | Native::ArrayBufferTransferToFixedLength
        | Native::ArrayBufferTransferToImmutable
        | Native::ArrayBufferByteLengthGetter
        | Native::ArrayBufferDetachedGetter
        | Native::ArrayBufferImmutableGetter
        | Native::ArrayBufferMaxByteLengthGetter
        | Native::ArrayBufferResizableGetter
        | Native::SharedArrayBufferByteLengthGetter
        | Native::SharedArrayBufferGrowableGetter
        | Native::SharedArrayBufferMaxByteLengthGetter
        | Native::ArrayBufferSpecies => 0.0,
        Native::ArraySpecies => 0.0,
        Native::ArrayOf
        | Native::ArrayPop
        | Native::ArrayShift
        | Native::ArrayFlat
        | Native::ArrayReverse
        | Native::ArrayToReversed
        | Native::ArrayToString
        | Native::ArrayToLocaleString
        | Native::ArrayKeys
        | Native::ArrayValues
        | Native::ArrayEntries => 0.0,
        Native::ArrayJoin
        | Native::ArrayConcat
        | Native::ArrayMap
        | Native::ArrayFilter
        | Native::ArraySome
        | Native::ArrayEvery
        | Native::ArrayFind
        | Native::ArrayFindIndex
        | Native::ArrayFindLast
        | Native::ArrayFindLastIndex
        | Native::ArrayGroup
        | Native::ArrayGroupToMap
        | Native::ArrayIncludes
        | Native::ArrayIndexOf
        | Native::ArrayLastIndexOf
        | Native::ArrayFlatMap
        | Native::ArrayAt
        | Native::ArraySort
        | Native::ArrayForEach
        | Native::ArrayReduce
        | Native::ArrayReduceRight
        | Native::ArrayPush
        | Native::ArrayUnshift
        | Native::ArrayFill
        | Native::ArrayToSorted => 1.0,
        Native::ArrayCopyWithin
        | Native::ArrayToSpliced
        | Native::ArrayWith
        | Native::ArraySlice
        | Native::ArraySplice => 2.0,
        Native::ObjectKeys
        | Native::ObjectValues
        | Native::ObjectEntries
        | Native::ObjectGetOwnPropertyNames
        | Native::ObjectGetOwnPropertySymbols
        | Native::ObjectGetOwnPropertyDescriptors
        | Native::ObjectFreeze
        | Native::ObjectSeal
        | Native::ObjectPreventExtensions
        | Native::ObjectIsFrozen
        | Native::ObjectIsSealed
        | Native::ObjectIsExtensible
        | Native::ObjectGetPrototypeOf => 1.0,
        Native::ObjectHasOwn | Native::ForInKeyIsEnumerable => 2.0,
        Native::ObjectGetOwnPropertyDescriptor | Native::ObjectIs => 2.0,
        Native::ObjectCreate => 2.0,
        Native::ObjectDefineProperties | Native::ObjectAssign => 2.0,
        Native::ObjectFromEntries => 1.0,
        Native::ObjectDefineProperty => 3.0,
        Native::ObjectGroupBy => 2.0,
        Native::ObjectSetPrototypeOf => 2.0,
        Native::JsonParse => 2.0,
        Native::JsonStringify => 3.0,
        Native::JsonRawJson | Native::JsonIsRawJson => 1.0,
        Native::RegExpSymbolMatch => 1.0,
        Native::RegExpSpecies => 0.0,
        Native::ReflectHas | Native::ReflectApply => 2.0,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PromiseReaction {
    pub(super) on_fulfilled: Value,
    pub(super) on_rejected: Value,
    pub(super) next: Value,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct FinallyReaction {
    pub(super) handler: Value,
    pub(super) next: Value,
}

#[derive(Clone, Debug)]
pub(super) struct PromiseRecord {
    pub(super) state: PromiseState,
    pub(super) result: Value,
    pub(super) reactions: Vec<PromiseReaction>,
    pub(super) finally_reactions: Vec<FinallyReaction>,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct PromiseJob {
    pub(super) handler: Value,
    pub(super) next: Value,
    pub(super) rejected: bool,
    pub(super) value: Value,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ThenableJob {
    pub(super) then: Value,
    pub(super) thenable: Value,
    pub(super) promise: Value,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FinallyJob {
    pub(super) handler: Value,
    pub(super) next: Value,
    pub(super) rejected: bool,
    pub(super) value: Value,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FinallyContinuationJob {
    pub(super) next: Value,
    pub(super) original_rejected: bool,
    pub(super) cleanup_rejected: bool,
    pub(super) value: Value,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FinallyHandlerCallback {
    pub(super) handler: Value,
    pub(super) constructor: Value,
    pub(super) original_rejected: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FinallyContinuationCallback {
    pub(super) original_rejected: bool,
    pub(super) original: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AggregateMode {
    All,
    AllKeyed,
    Race,
    AllSettled,
    AllSettledKeyed,
    Any,
}

impl AggregateMode {
    pub(super) const fn is_all(self) -> bool {
        matches!(self, Self::All | Self::AllKeyed)
    }

    pub(super) const fn is_all_settled(self) -> bool {
        matches!(self, Self::AllSettled | Self::AllSettledKeyed)
    }

    pub(super) const fn is_keyed(self) -> bool {
        matches!(self, Self::AllKeyed | Self::AllSettledKeyed)
    }
}

#[derive(Clone, Debug)]
pub(super) struct AggregateRecord {
    pub(super) mode: AggregateMode,
    pub(super) output: Value,
    pub(super) resolve: Value,
    pub(super) reject: Value,
    pub(super) remaining: usize,
    pub(super) values: Vec<Value>,
    pub(super) called: Vec<bool>,
    pub(super) keys: Option<Vec<Value>>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AggregateJob {
    pub(super) aggregate: Value,
    pub(super) index: usize,
    pub(super) rejected: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AsyncResumeJob {
    pub(super) continuation: ContinuationId,
    pub(super) promise: Value,
    pub(super) generator: Option<Value>,
    pub(super) rejected: bool,
    pub(super) yielded: bool,
}

#[derive(Clone, Debug)]
pub(super) struct DynamicImportJob {
    pub(super) cache_key: String,
    pub(super) module: ModuleSource,
    pub(super) promises: Vec<Value>,
}

pub(super) struct PromiseRuntime {
    pub(super) proto: Value,
    pub(super) records: FxHashMap<Value, PromiseRecord>,
    pub(super) jobs: FxHashMap<Value, PromiseJob>,
    pub(super) thenable_jobs: FxHashMap<Value, ThenableJob>,
    pub(super) finally_jobs: FxHashMap<Value, FinallyJob>,
    pub(super) finally_continuation_jobs: FxHashMap<Value, FinallyContinuationJob>,
    pub(super) finally_handler_callbacks: FxHashMap<Value, FinallyHandlerCallback>,
    pub(super) finally_continuation_callbacks: FxHashMap<Value, FinallyContinuationCallback>,
    pub(super) aggregates: FxHashMap<Value, AggregateRecord>,
    pub(super) aggregate_jobs: FxHashMap<Value, AggregateJob>,
    pub(super) reaction_capabilities: FxHashMap<Value, (Value, Value)>,
    pub(super) async_resume_jobs: FxHashMap<Value, AsyncResumeJob>,
    pub(super) modules: FxHashMap<String, ModuleRecord>,
    pub(super) dynamic_import_jobs: Vec<DynamicImportJob>,
    pub(super) async_module_order: VecDeque<String>,
    pub(super) module_sources: FxHashMap<std::path::PathBuf, Value>,
    pub(super) waiting_static_modules: Vec<ModuleSource>,
    pub(super) active_native: Vec<Value>,
}

impl Default for PromiseRuntime {
    fn default() -> Self {
        Self {
            proto: Value::NULL,
            records: FxHashMap::default(),
            jobs: FxHashMap::default(),
            thenable_jobs: FxHashMap::default(),
            finally_jobs: FxHashMap::default(),
            finally_continuation_jobs: FxHashMap::default(),
            finally_handler_callbacks: FxHashMap::default(),
            finally_continuation_callbacks: FxHashMap::default(),
            aggregates: FxHashMap::default(),
            aggregate_jobs: FxHashMap::default(),
            reaction_capabilities: FxHashMap::default(),
            async_resume_jobs: FxHashMap::default(),
            modules: FxHashMap::default(),
            dynamic_import_jobs: Vec::new(),
            async_module_order: VecDeque::new(),
            module_sources: FxHashMap::default(),
            waiting_static_modules: Vec::new(),
            active_native: vec![],
        }
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn abstract_module_source_to_string_tag(&mut self, receiver: Value) -> Value {
        if !self.is_object_like(receiver)
            || !self
                .promise
                .module_sources
                .values()
                .any(|module_source| *module_source == receiver)
        {
            return Value::UNDEFINED;
        }
        self.heap.alloc(Cell::String("ModuleSource".into()))
    }

    pub(super) fn call_native_guarded(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
        callee: Value,
    ) -> Result<Value, JsError> {
        let realm = match self.heap.get(callee) {
            Some(Cell::Function { realm, .. }) => *realm,
            _ => self.realm.globals,
        };
        let previous_global = std::mem::replace(&mut self.realm.globals, realm);
        self.promise.active_native.push(callee);
        let result = self.call_native(p, native, this, args);
        self.promise.active_native.pop();
        self.realm.globals = previous_global;
        result
    }

    pub(super) fn native_with_env(&mut self, kind: Native, env: Value) -> Value {
        self.native_with_realm(kind, env, self.realm.globals)
    }

    pub(super) fn native_with_realm(&mut self, kind: Native, env: Value, realm: Value) -> Value {
        let function = self.heap.alloc(Cell::Function {
            object: Box::new(Self::empty_object(self.function_proto)),
            kind: FunctionKind::Native(kind),
            env,
            realm,
        });
        let promise_resolver = !env.is_null()
            && matches!(
                kind,
                Native::PromiseResolve
                    | Native::PromiseReject
                    | Native::PromiseAggregateJob
                    | Native::PromiseFinallyJob
            );
        let length = promise_resolver
            .then_some(1.0)
            .or_else(|| native_length(kind));
        if let Some(length) = length {
            let atom = self.intern_atom("length");
            let _ = self.set_property(function, atom, Value::number(length));
            self.set_property_attributes(
                function,
                property_key::PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        if promise_resolver {
            let atom = self.intern_atom("name");
            let empty_name = self.heap.alloc(Cell::String("".into()));
            let _ = self.set_property(function, atom, empty_name);
            self.set_property_attributes(
                function,
                property_key::PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        function
    }

    pub(super) fn install_promise(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.promise.proto = self.object();
        let promise = self.native_value(Native::Promise);
        self.set_builtin_function_name(promise, "Promise")?;
        self.set_named(program, promise, "prototype", self.promise.proto)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            promise,
            property_key::PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_named(program, self.promise.proto, "constructor", promise)?;
        let constructor = self.intern_atom("constructor");
        self.set_property_attributes(
            self.promise.proto,
            property_key::PropertyKey::string(constructor),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_named(program, self.promise.proto, "then", Native::PromiseThen)?;
        self.set_builtin_named(program, self.promise.proto, "catch", Native::PromiseCatch)?;
        self.set_builtin_named(
            program,
            self.promise.proto,
            "finally",
            Native::PromiseFinally,
        )?;
        self.install_builtin_to_string_tag(self.promise.proto, "Promise")?;
        self.set_builtin_named(program, promise, "resolve", Native::PromiseResolve)?;
        self.set_builtin_named(program, promise, "reject", Native::PromiseReject)?;
        let with_resolvers = self.native_value(Native::PromiseWithResolvers);
        self.set_named(program, promise, "withResolvers", with_resolvers)?;
        let name_atom = self.intern_atom("name");
        let name = self.heap.alloc(Cell::String("withResolvers".into()));
        self.set_named(program, with_resolvers, "name", name)?;
        self.set_property_attributes(
            with_resolvers,
            property_key::PropertyKey::string(name_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_named(program, promise, "all", Native::PromiseAll)?;
        self.set_builtin_named(program, promise, "allKeyed", Native::PromiseAllKeyed)?;
        self.set_builtin_named(program, promise, "race", Native::PromiseRace)?;
        self.set_builtin_named(program, promise, "allSettled", Native::PromiseAllSettled)?;
        self.set_builtin_named(
            program,
            promise,
            "allSettledKeyed",
            Native::PromiseAllSettledKeyed,
        )?;
        self.set_builtin_named(program, promise, "any", Native::PromiseAny)?;
        if let Some(species) = self.well_known_symbols.get("species").copied() {
            let getter = self.native_value(Native::PromiseSpeciesGetter);
            self.set_builtin_function_name(getter, "get [Symbol.species]")?;
            self.set_symbol_property(promise, species, Value::UNDEFINED)?;
            self.set_property_attributes(
                promise,
                property_key::PropertyKey::symbol(species),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            );
        }
        self.global(program, "Promise", promise)
    }

    pub(super) fn promise_object(&mut self) -> Value {
        let promise = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.promise.proto)));
        self.promise.records.insert(
            promise,
            PromiseRecord {
                state: PromiseState::Pending,
                result: Value::UNDEFINED,
                reactions: vec![],
                finally_reactions: vec![],
            },
        );
        promise
    }

    pub(super) fn construct_promise(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let executor = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(executor) {
            return Err(self.type_error(p, "Promise resolver is not a function".into()));
        }
        let promise = self.promise_object();
        let resolve = self.native_with_env(Native::PromiseResolve, promise);
        let reject = self.native_with_env(Native::PromiseReject, promise);
        if let Err(error) = self.call_value(p, executor, Value::UNDEFINED, &[resolve, reject]) {
            self.promise_settle(
                p,
                promise,
                PromiseState::Rejected,
                error.thrown_value().unwrap_or(Value::UNDEFINED),
            )?;
        }
        Ok(promise)
    }

    fn promise_with_resolvers(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
    ) -> Result<Value, JsError> {
        let (promise, resolve, reject) = self.new_promise_capability(p, constructor)?;
        let result = self.object();
        self.set_named(p, result, "promise", promise)?;
        self.set_named(p, result, "resolve", resolve)?;
        self.set_named(p, result, "reject", reject)?;
        Ok(result)
    }

    pub(super) fn new_promise_capability(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
    ) -> Result<(Value, Value, Value), JsError> {
        if !self.is_constructable(p, constructor) {
            return Err(self.type_error(
                p,
                "Promise.withResolvers receiver is not a constructor".into(),
            ));
        }
        let state = self.object();
        let resolve_atom = self.intern_atom(PROMISE_CAPABILITY_RESOLVE);
        let reject_atom = self.intern_atom(PROMISE_CAPABILITY_REJECT);
        self.set_property(state, resolve_atom, Value::UNDEFINED)?;
        self.set_property(state, reject_atom, Value::UNDEFINED)?;
        let executor = self.native_with_env(Native::PromiseCapabilityExecutor, state);
        let promise = self.construct_value(p, constructor, &[executor])?;
        let resolve = self
            .own_property(state, resolve_atom)
            .filter(|value| self.is_function(*value))
            .ok_or_else(|| {
                self.type_error(p, "Promise capability resolve is not callable".into())
            })?;
        let reject = self
            .own_property(state, reject_atom)
            .filter(|value| self.is_function(*value))
            .ok_or_else(|| {
                self.type_error(p, "Promise capability reject is not callable".into())
            })?;
        Ok((promise, resolve, reject))
    }

    fn promise_capability_executor(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let state = self
            .active_native_env()
            .ok_or_else(|| self.type_error(p, "invalid Promise capability executor".into()))?;
        let resolve_atom = self.intern_atom(PROMISE_CAPABILITY_RESOLVE);
        let reject_atom = self.intern_atom(PROMISE_CAPABILITY_REJECT);
        if self
            .own_property(state, resolve_atom)
            .is_some_and(|value| !value.is_undefined())
            || self
                .own_property(state, reject_atom)
                .is_some_and(|value| !value.is_undefined())
        {
            return Err(self.type_error(p, "Promise capability executor was already called".into()));
        }
        let resolve = args.first().copied().unwrap_or(Value::UNDEFINED);
        let reject = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        self.set_property(state, resolve_atom, resolve)?;
        self.set_property(state, reject_atom, reject)?;
        Ok(Value::UNDEFINED)
    }

    pub(super) fn call_promise_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::DynamicImport => self.dynamic_import(p, args),
            Native::AbstractModuleSource => {
                Err(self.type_error(p, "AbstractModuleSource cannot be called".into()))
            }
            Native::Promise => Err(JsError(
                "Promise constructor must be called with new".into(),
            )),
            Native::PromiseWithResolvers => self.promise_with_resolvers(p, this),
            Native::PromiseCapabilityExecutor => self.promise_capability_executor(p, args),
            Native::PromiseSpeciesGetter => Ok(this),
            Native::PromiseResolve => {
                if let Some(promise) = self.active_native_env() {
                    self.promise_resolve_value(
                        p,
                        promise,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                    )?;
                    Ok(Value::UNDEFINED)
                } else {
                    if !self.is_constructable(p, this) {
                        return Err(self.type_error(
                            p,
                            "Promise.resolve receiver is not a constructor".into(),
                        ));
                    }
                    if let Some(value) = args.first().copied()
                        && self.promise.records.contains_key(&value)
                    {
                        let constructor_atom = self.intern_atom("constructor");
                        if self.get_property(p, value, constructor_atom)? == this {
                            return Ok(value);
                        }
                    }
                    let (promise, resolve, _) = self.new_promise_capability(p, this)?;
                    self.call_value(
                        p,
                        resolve,
                        Value::UNDEFINED,
                        &[args.first().copied().unwrap_or(Value::UNDEFINED)],
                    )?;
                    Ok(promise)
                }
            }
            Native::PromiseReject => {
                if let Some(promise) = self.active_native_env() {
                    self.promise_settle(
                        p,
                        promise,
                        PromiseState::Rejected,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                    )?;
                    Ok(Value::UNDEFINED)
                } else {
                    let (promise, _, reject) = self.new_promise_capability(p, this)?;
                    self.call_value(
                        p,
                        reject,
                        Value::UNDEFINED,
                        &[args.first().copied().unwrap_or(Value::UNDEFINED)],
                    )?;
                    Ok(promise)
                }
            }
            Native::PromiseThen => self.promise_then(
                p,
                this,
                args.first().copied().unwrap_or(Value::UNDEFINED),
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::PromiseCatch => self.promise_catch(p, this, args),
            Native::PromiseFinally => self.promise_finally(p, this, args),
            Native::PromiseFinallyHandler => self.promise_finally_handler(p, args),
            Native::PromiseFinallyContinuationHandler => {
                self.promise_finally_continuation_handler(p, args)
            }
            Native::PromiseAll => self.promise_aggregate(p, this, args, AggregateMode::All),
            Native::PromiseAllKeyed => {
                self.promise_aggregate(p, this, args, AggregateMode::AllKeyed)
            }
            Native::PromiseRace => self.promise_aggregate(p, this, args, AggregateMode::Race),
            Native::PromiseAllSettled => {
                self.promise_aggregate(p, this, args, AggregateMode::AllSettled)
            }
            Native::PromiseAllSettledKeyed => {
                self.promise_aggregate(p, this, args, AggregateMode::AllSettledKeyed)
            }
            Native::PromiseAny => self.promise_aggregate(p, this, args, AggregateMode::Any),
            Native::PromiseReactionJob => {
                self.promise_reaction_job(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::PromiseThenableJob => self.promise_thenable_job(p),
            Native::PromiseFinallyJob => self.promise_finally_job(p, args),
            Native::PromiseFinallyContinuationJob => self.promise_finally_continuation_job(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::PromiseAggregateJob => {
                self.promise_aggregate_job(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::PromiseAsyncResumeJob => {
                self.promise_async_resume_job(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ArrayFromAsyncFulfilled | Native::ArrayFromAsyncRejected => self
                .array_from_async_reaction(
                    p,
                    native == Native::ArrayFromAsyncFulfilled,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                ),
            Native::AsyncFromSyncValue => self.async_from_sync_value(args),
            Native::AsyncGeneratorDelegateFulfilled => self.async_generator_delegate_fulfilled(
                p,
                this,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::AsyncGeneratorDelegateRejected => self.async_generator_delegate_rejected(
                p,
                this,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::AsyncGeneratorReturnFulfilled => self.async_generator_return_fulfilled(p, args),
            Native::AsyncGeneratorReturnRejected => self.async_generator_return_rejected(p, args),
            _ => unreachable!(),
        }
    }

    fn dynamic_import(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let promise = self.promise_object();
        let specifier = args.first().copied().unwrap_or(Value::UNDEFINED);
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let phase = args
            .get(2)
            .and_then(|value| value.as_number())
            .and_then(crate::bytecode::ModuleRequestPhase::from_runtime_value)
            .unwrap_or(crate::bytecode::ModuleRequestPhase::Evaluation);
        let operation = (|| {
            let specifier = self.to_string(p, specifier)?;
            let module_type = self.validate_dynamic_import_options(p, options)?;
            let resolution = self.host.resolve_dynamic_import(&p.source_name, &specifier);
            match resolution {
                Err(message) => return Err(self.type_error(p, message)),
                Ok(Some(module)) => {
                    if phase == crate::bytecode::ModuleRequestPhase::Source {
                        return self.syntax_error_result(
                            p,
                            &format!(
                                "Source phase import object is not defined for module '{}'",
                                module.name
                            ),
                        );
                    }
                    let module_type = module_type.as_deref().unwrap_or("javascript");
                    let cache_key = module_cache_key(&module.name, module_type);
                    if let Some(outcome) = self
                        .promise
                        .modules
                        .get(&cache_key)
                        .map(|record| record.outcome)
                    {
                        match outcome {
                            ModuleOutcome::Evaluated(namespace) => {
                                let namespace =
                                    if phase == crate::bytecode::ModuleRequestPhase::Defer {
                                        let deferred = self
                                            .promise
                                            .modules
                                            .get(&cache_key)
                                            .and_then(ModuleRecord::deferred_namespace);
                                        match deferred {
                                            Some(namespace) => namespace,
                                            None => {
                                                let namespace =
                                                    self.deferred_module_namespace(p, &module)?;
                                                self.promise
                                                    .modules
                                                    .get_mut(&cache_key)
                                                    .expect("module record found above")
                                                    .cache_deferred_namespace(namespace);
                                                namespace
                                            }
                                        }
                                    } else {
                                        namespace
                                    };
                                self.promise_resolve_value(p, promise, namespace)?;
                                return Ok(Value::UNDEFINED);
                            }
                            ModuleOutcome::Deferred(namespace)
                                if phase == crate::bytecode::ModuleRequestPhase::Defer =>
                            {
                                self.promise_resolve_value(p, promise, namespace)?;
                                return Ok(Value::UNDEFINED);
                            }
                            ModuleOutcome::Deferred(namespace) => {
                                let namespace =
                                    self.evaluate_deferred_module_namespace(p, namespace)?;
                                self.promise_resolve_value(p, promise, namespace)?;
                                return Ok(Value::UNDEFINED);
                            }
                            ModuleOutcome::Errored(reason) => {
                                if phase == crate::bytecode::ModuleRequestPhase::Defer
                                    && module_type == "javascript"
                                {
                                    let namespace = self
                                        .promise
                                        .modules
                                        .get(&cache_key)
                                        .and_then(ModuleRecord::deferred_namespace);
                                    let namespace = match namespace {
                                        Some(namespace) => namespace,
                                        None => {
                                            let namespace =
                                                self.deferred_module_namespace(p, &module)?;
                                            self.promise
                                                .modules
                                                .get_mut(&cache_key)
                                                .expect("module record found above")
                                                .cache_deferred_namespace(namespace);
                                            namespace
                                        }
                                    };
                                    self.promise_resolve_value(p, promise, namespace)?;
                                } else {
                                    self.promise_settle(
                                        p,
                                        promise,
                                        PromiseState::Rejected,
                                        reason,
                                    )?;
                                }
                                return Ok(Value::UNDEFINED);
                            }
                            ModuleOutcome::Pending(_) => {
                                let joined = self
                                    .promise
                                    .modules
                                    .get_mut(&cache_key)
                                    .expect("module record found above")
                                    .add_waiter(promise);
                                debug_assert!(joined);
                                return Ok(Value::UNDEFINED);
                            }
                        }
                    }
                    if let Some(namespace) = self.root_module_namespace(p, &module)? {
                        self.promise
                            .modules
                            .insert(cache_key, ModuleRecord::evaluating_root(namespace, promise));
                        return Ok(Value::UNDEFINED);
                    }
                    if module_type == "javascript"
                        && phase == crate::bytecode::ModuleRequestPhase::Evaluation
                        && self.deferred_dependency_batch
                    {
                        if let Some(job) = self
                            .promise
                            .dynamic_import_jobs
                            .iter_mut()
                            .find(|job| job.cache_key == cache_key)
                        {
                            job.promises.push(promise);
                        } else {
                            self.promise.dynamic_import_jobs.push(DynamicImportJob {
                                cache_key,
                                module,
                                promises: vec![promise],
                            });
                        }
                        return Ok(Value::UNDEFINED);
                    }
                    if module_type == "javascript"
                        && phase == crate::bytecode::ModuleRequestPhase::Defer
                    {
                        let mut seen = FxHashSet::default();
                        let mut asynchronous = Vec::new();
                        self.gather_async_transitive_dependencies(
                            p,
                            &module,
                            &mut seen,
                            &mut asynchronous,
                        )?;
                        if asynchronous.is_empty() {
                            let namespace = self.deferred_module_namespace(p, &module)?;
                            self.promise
                                .modules
                                .insert(cache_key, ModuleRecord::deferred(namespace));
                            self.promise_resolve_value(p, promise, namespace)?;
                            return Ok(Value::UNDEFINED);
                        }
                        let mut active = ModuleEvaluationStack::default();
                        for dependency in asynchronous {
                            self.evaluate_static_module_source(p, dependency, &mut active)?;
                        }
                        let entry_has_tla =
                            crate::Engine::static_module_plan(&module.source, &module.name)
                                .is_some_and(|plan| plan.has_top_level_await);
                        if !entry_has_tla {
                            let namespace = self.deferred_module_namespace(p, &module)?;
                            self.promise
                                .modules
                                .insert(cache_key, ModuleRecord::deferred(namespace));
                            self.promise_resolve_value(p, promise, namespace)?;
                            return Ok(Value::UNDEFINED);
                        }
                        if let Some(ModuleOutcome::Evaluated(namespace)) = self
                            .promise
                            .modules
                            .get(&cache_key)
                            .map(|record| record.outcome)
                        {
                            self.promise_resolve_value(p, promise, namespace)?;
                            return Ok(Value::UNDEFINED);
                        }
                    }
                    if module_type == "javascript"
                        && phase == crate::bytecode::ModuleRequestPhase::Evaluation
                    {
                        let mut active = ModuleEvaluationStack::default();
                        let outer_batch =
                            std::mem::replace(&mut self.deferred_dependency_batch, true);
                        let evaluation =
                            self.evaluate_static_module_source(p, module.clone(), &mut active);
                        self.deferred_dependency_batch = outer_batch;
                        evaluation?;
                        match self
                            .promise
                            .modules
                            .get(&cache_key)
                            .map(|record| record.outcome)
                        {
                            Some(ModuleOutcome::Evaluated(namespace)) => {
                                self.promise_resolve_value(p, promise, namespace)?;
                            }
                            Some(ModuleOutcome::Errored(reason)) => {
                                self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                            }
                            Some(ModuleOutcome::Pending(_)) => {
                                let joined = self
                                    .promise
                                    .modules
                                    .get_mut(&cache_key)
                                    .expect("module record found above")
                                    .add_waiter(promise);
                                debug_assert!(joined);
                            }
                            Some(ModuleOutcome::Deferred(_)) | None => {
                                return Err(self.type_error(
                                    p,
                                    "dynamic module evaluation did not create a module record"
                                        .into(),
                                ));
                            }
                        }
                        return Ok(Value::UNDEFINED);
                    }
                    let mut record = ModuleRecord::loading(promise);
                    let linked = record.begin_linking();
                    debug_assert!(linked);
                    let evaluating = record.begin_evaluation();
                    debug_assert!(evaluating);
                    self.promise.modules.insert(cache_key.clone(), record);
                    match self.evaluate_dynamic_module(p, &module, module_type, phase) {
                        Ok(namespace) => {
                            let waiters = self
                                .promise
                                .modules
                                .get_mut(&cache_key)
                                .expect("module record inserted above")
                                .evaluate(namespace)
                                .expect("module record is evaluating");
                            for waiter in waiters {
                                self.promise_resolve_value(p, waiter, namespace)?;
                            }
                        }
                        Err(error) => {
                            let reason = error.thrown_value().unwrap_or_else(|| {
                                self.heap.alloc(Cell::Error(error.into_message()))
                            });
                            let waiters = self
                                .promise
                                .modules
                                .get_mut(&cache_key)
                                .expect("module record inserted above")
                                .fail(reason)
                                .expect("module record is pending");
                            for waiter in waiters {
                                self.promise_settle(p, waiter, PromiseState::Rejected, reason)?;
                            }
                        }
                    }
                    return Ok(Value::UNDEFINED);
                }
                Ok(None) => {}
            }
            Err(self.type_error(p, "host did not resolve dynamic import module".into()))
        })();
        if let Err(error) = operation {
            let reason = error
                .thrown_value()
                .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
            self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
        }
        Ok(promise)
    }

    fn root_module_namespace(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<Option<Value>, JsError> {
        if !p.module
            || self.active_program != ProgramId::MAIN
            || !crate::module_identity::same_name(&module.name, &p.source_name)
        {
            return Ok(None);
        }
        let Some(plan) = p.module_link_plan.as_ref() else {
            return Ok(None);
        };
        let Some(locals) = self.root_module_local_exports(plan.locals.clone()) else {
            return Ok(None);
        };
        let mut active = ActiveModuleExports::default();
        active.insert(
            crate::module_identity::normalize(std::path::Path::new(&module.name)),
            locals.clone(),
        );
        match self.resolve_static_module_links(
            p,
            module,
            locals,
            plan.reexports.clone(),
            &mut active,
        )? {
            StaticModuleGraph::Linked {
                exports,
                incomplete: false,
                ..
            } => self.module_namespace_from_static(exports).map(Some),
            StaticModuleGraph::Linked {
                incomplete: true, ..
            }
            | StaticModuleGraph::LinkError => self
                .syntax_error_result(p, "module export could not be resolved unambiguously")
                .map(Some),
            StaticModuleGraph::Unsupported => Ok(None),
        }
    }

    fn root_module_local_exports(
        &self,
        locals: Vec<(String, String)>,
    ) -> Option<Vec<(String, StaticModuleValue)>> {
        let residual = self.programs.get(ProgramId::MAIN)?;
        let root = residual.functions.first()?;
        locals
            .into_iter()
            .map(|(local, exported)| {
                let local_atom = residual.atoms.iter().position(|name| name == local)?;
                let slot = root
                    .local_atoms
                    .iter()
                    .position(|atom| *atom as usize == local_atom)?;
                Some((
                    exported,
                    StaticModuleValue::Binding {
                        program: ProgramId::MAIN,
                        slot: u16::try_from(slot).ok()?,
                        value: self
                            .programs
                            .module_environment(ProgramId::MAIN)
                            .and_then(|environment| match self.heap.get(environment) {
                                Some(Cell::Environment { slots, .. }) => slots.get(slot).copied(),
                                _ => None,
                            })
                            .filter(|value| !value.is_deleted())
                            .unwrap_or(Value::UNDEFINED),
                    },
                ))
            })
            .collect()
    }

    pub(super) fn finish_main_module(
        &mut self,
        p: &ResidualProgram,
        result: &Result<Value, JsError>,
    ) -> Result<(), JsError> {
        if !p.module {
            return Ok(());
        }
        let key = module_cache_key(&p.source_name, "javascript");
        if !self.promise.modules.contains_key(&key) {
            return Ok(());
        }
        match result {
            Ok(value)
                if p.functions
                    .first()
                    .is_some_and(|function| function.is_async) =>
            {
                self.finish_async_main_module(p, &key, *value)
            }
            Ok(_) => self.complete_main_module(p, &key),
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.to_string())));
                self.fail_main_module(p, &key, reason)
            }
        }
    }

    fn finish_async_main_module(
        &mut self,
        p: &ResidualProgram,
        key: &str,
        promise: Value,
    ) -> Result<(), JsError> {
        let Some(completion) = self.promise.records.get(&promise).cloned() else {
            return Err(self.type_error(p, "module completion promise is unavailable".into()));
        };
        match completion.state {
            PromiseState::Pending => {
                let Some(record) = self.promise.modules.get_mut(key) else {
                    return Ok(());
                };
                let Some(namespace) = record.pending_namespace() else {
                    return Err(self.type_error(p, "main module namespace is unavailable".into()));
                };
                record.begin_async_evaluation(namespace);
                record.track_evaluation_promise(promise);
                self.promise.async_module_order.push_back(key.to_owned());
                Ok(())
            }
            PromiseState::Fulfilled => self.complete_main_module(p, key),
            PromiseState::Rejected => {
                self.fail_main_module(p, key, completion.result)?;
                Err(JsError::thrown(
                    completion.result,
                    "main module evaluation rejected".into(),
                ))
            }
        }
    }

    fn complete_main_module(&mut self, p: &ResidualProgram, key: &str) -> Result<(), JsError> {
        let Some((namespace, waiters)) = self
            .promise
            .modules
            .get_mut(key)
            .and_then(ModuleRecord::evaluate_root)
        else {
            return Ok(());
        };
        for waiter in waiters {
            self.promise_resolve_value(p, waiter, namespace)?;
        }
        Ok(())
    }

    fn fail_main_module(
        &mut self,
        p: &ResidualProgram,
        key: &str,
        reason: Value,
    ) -> Result<(), JsError> {
        let waiters = self
            .promise
            .modules
            .get_mut(key)
            .and_then(|record| record.fail(reason));
        if let Some(waiters) = waiters {
            for waiter in waiters {
                self.promise_settle(p, waiter, PromiseState::Rejected, reason)?;
            }
        }
        Ok(())
    }

    fn evaluate_dynamic_module(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        module_type: &str,
        phase: crate::bytecode::ModuleRequestPhase,
    ) -> Result<Value, JsError> {
        if phase == crate::bytecode::ModuleRequestPhase::Source {
            return Ok(self.module_source_value(module));
        }
        match module_type {
            "bytes" => {
                let bytes = module.bytes.clone();
                let length = bytes.len();
                let kind = crate::heap::TypedArrayKind::Uint8;
                let buffer = self.heap.alloc(Cell::ArrayBuffer {
                    object: Self::empty_object(self.array_buffer_proto),
                    bytes: std::rc::Rc::new(bytes),
                    shared: false,
                    detached: false,
                    max_byte_length: length,
                    resizable: false,
                    immutable: true,
                });
                let value = self.heap.alloc(Cell::TypedArray {
                    kind,
                    object: Self::empty_object(self.typed_array_proto(kind)),
                    buffer,
                    offset: 0,
                    length,
                    length_tracking: false,
                });
                self.module_namespace_default(value)
            }
            "text" => {
                let source = self.heap.alloc(Cell::String(module.source.clone().into()));
                self.module_namespace_default(source)
            }
            "json" => {
                let source = self.heap.alloc(Cell::String(module.source.clone().into()));
                let value = self.json_parse(p, &[source])?;
                self.module_namespace_default(value)
            }
            "javascript" => self.evaluate_javascript_module(p, module),
            _ => Err(self.type_error(p, "unsupported dynamic import type".into())),
        }
    }

    fn module_source_value(&mut self, module: &ModuleSource) -> Value {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if let Some(value) = self.promise.module_sources.get(&identity) {
            return *value;
        }
        let prototype_atom = self.intern_atom("prototype");
        let constructor = self.native_value(Native::AbstractModuleSource);
        let prototype = self
            .own_property(constructor, prototype_atom)
            .unwrap_or(self.object_proto);
        let value = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.promise.module_sources.insert(identity, value);
        value
    }

    fn evaluate_javascript_module(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<Value, JsError> {
        if crate::Engine::static_module_has_early_error(&module.source) {
            return self.syntax_error_result(p, "module source has an early error");
        }
        if let Some(plan) = crate::Engine::static_module_plan(&module.source, &module.name) {
            let mut active = ModuleEvaluationStack::default();
            active.enter(crate::module_identity::normalize(std::path::Path::new(
                &module.name,
            )));
            self.evaluate_module_requests(p, &module.name, &plan.requests, &mut active)?;
        }
        self.evaluate_javascript_module_body(p, module)
    }

    fn gather_async_transitive_dependencies(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        seen: &mut FxHashSet<std::path::PathBuf>,
        output: &mut Vec<ModuleSource>,
    ) -> Result<(), JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if !seen.insert(identity) {
            return Ok(());
        }
        let Some(plan) = crate::Engine::static_module_plan(&module.source, &module.name) else {
            return Err(self.type_error(p, "deferred module metadata is unavailable".into()));
        };
        if plan.has_top_level_await {
            output.push(module.clone());
            return Ok(());
        }
        for request in plan
            .requests
            .iter()
            .filter(|request| request.phase == crate::bytecode::ModuleRequestPhase::Evaluation)
        {
            let dependency = self
                .host
                .resolve_dynamic_import(&module.name, &request.source)
                .map_err(|message| self.type_error(p, message))?;
            let Some(dependency) = dependency else {
                return Err(self.type_error(p, "static module request was not resolved".into()));
            };
            self.gather_async_transitive_dependencies(p, &dependency, seen, output)?;
        }
        Ok(())
    }

    fn deferred_module_namespace(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<Value, JsError> {
        let Some(plan) = crate::Engine::static_module_plan(&module.source, &module.name) else {
            return Err(self.type_error(p, "deferred module metadata is unavailable".into()));
        };
        let mut names = plan
            .link_plan
            .into_iter()
            .flat_map(|plan| plan.locals)
            .map(|(_, exported)| exported)
            .collect::<Vec<_>>();
        if let Some(exports) = crate::Engine::static_module_exports(&module.source, &module.name) {
            names.extend(exports.into_iter().map(|(name, _)| name));
        }
        names.sort_by_key(|name| name.encode_utf16().collect::<Vec<_>>());
        names.dedup();
        let namespace = self.module_namespace_with_tag(
            names
                .into_iter()
                .map(|name| (name, Value::UNDEFINED))
                .collect(),
            "Deferred Module",
        )?;
        let Some(object) = self.object_data_mut(namespace) else {
            return Err(self.type_error(p, "module namespace allocation failed".into()));
        };
        object.deferred_module = Some(module.clone());
        Ok(namespace)
    }

    pub(super) fn evaluate_deferred_module_namespace(
        &mut self,
        p: &ResidualProgram,
        namespace: Value,
    ) -> Result<Value, JsError> {
        let Some(module) = self
            .object_data(namespace)
            .and_then(|object| object.deferred_module.clone())
        else {
            return Ok(namespace);
        };
        let cache_key = module_cache_key(&module.name, "javascript");
        match self
            .promise
            .modules
            .get(&cache_key)
            .map(|record| record.outcome)
        {
            Some(ModuleOutcome::Errored(reason)) => {
                return Err(JsError::thrown(
                    reason,
                    "deferred module evaluation failed".into(),
                ));
            }
            Some(ModuleOutcome::Evaluated(evaluated)) => {
                self.copy_module_namespace(evaluated, namespace, p)?;
                return Ok(namespace);
            }
            Some(ModuleOutcome::Pending(ModulePhase::Evaluating)) => {
                return Err(self.type_error(
                    p,
                    "deferred module is not ready for synchronous evaluation".into(),
                ));
            }
            _ => {}
        }
        if !self.ready_for_sync_execution(p, &module, &mut FxHashSet::default())? {
            return Err(self.type_error(
                p,
                "deferred module is not ready for synchronous evaluation".into(),
            ));
        }
        let Some(record) = self.promise.modules.get_mut(&cache_key) else {
            return Err(self.type_error(p, "deferred module record is unavailable".into()));
        };
        if record.begin_deferred_evaluation().is_none() {
            return Ok(namespace);
        }
        let result = self.evaluate_javascript_module(p, &module);
        match result {
            Ok(evaluated) => {
                self.copy_module_namespace(evaluated, namespace, p)?;
                self.settle_static_module(p, &cache_key, Ok(evaluated))?;
                Ok(namespace)
            }
            Err(error) => {
                let settled = self.settle_static_module(p, &cache_key, Err(error));
                if let Some(object) = self.object_data_mut(namespace) {
                    object.deferred_module = Some(module);
                }
                settled?;
                Ok(namespace)
            }
        }
    }

    pub(super) fn evaluate_deferred_namespace_for_key(
        &mut self,
        p: &ResidualProgram,
        namespace: Value,
        key: Option<crate::vm::property_key::PropertyKey>,
    ) -> Result<(), JsError> {
        let triggers_evaluation = match key {
            None => true,
            Some(crate::vm::property_key::PropertyKey::String(atom)) => {
                self.atom_name(atom) != "then"
            }
            Some(
                crate::vm::property_key::PropertyKey::Symbol(_)
                | crate::vm::property_key::PropertyKey::Private(_),
            ) => false,
        };
        if triggers_evaluation
            && self
                .object_data(namespace)
                .is_some_and(|object| object.deferred_module.is_some())
        {
            self.evaluate_deferred_module_namespace(p, namespace)?;
        }
        Ok(())
    }

    fn ready_for_sync_execution(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        seen: &mut FxHashSet<std::path::PathBuf>,
    ) -> Result<bool, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if !seen.insert(identity) {
            return Ok(true);
        }
        match self
            .promise
            .modules
            .get(&module_cache_key(&module.name, "javascript"))
            .map(|record| record.outcome)
        {
            Some(ModuleOutcome::Evaluated(_)) | Some(ModuleOutcome::Errored(_)) => return Ok(true),
            Some(ModuleOutcome::Pending(_)) => return Ok(false),
            _ => {}
        }
        let Some(plan) = crate::Engine::static_module_plan(&module.source, &module.name) else {
            return Ok(false);
        };
        if plan.has_top_level_await {
            return Ok(false);
        }
        for request in plan.requests {
            let dependency = self
                .host
                .resolve_dynamic_import(&module.name, &request.source)
                .map_err(|message| self.type_error(p, message))?
                .ok_or_else(|| {
                    self.type_error(p, "static module request was not resolved".into())
                })?;
            if request.module_type.as_deref().unwrap_or("javascript") != "javascript" {
                continue;
            }
            if !self.ready_for_sync_execution(p, &dependency, seen)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn copy_module_namespace(
        &mut self,
        source: Value,
        target: Value,
        p: &ResidualProgram,
    ) -> Result<(), JsError> {
        let Some(source) = self.object_data(source).cloned() else {
            return Err(self.type_error(p, "evaluated module namespace is invalid".into()));
        };
        if let Some(target) = self.object_data_mut(target) {
            target.proto = source.proto;
            target.properties = source.properties;
            target.module_namespace = source.module_namespace;
            target.module_bindings = source.module_bindings;
            target.deferred_module = None;
        }
        Ok(())
    }

    fn evaluate_javascript_module_body(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<Value, JsError> {
        if let Some(exports) = crate::Engine::static_module_exports(&module.source, &module.name) {
            let exports = exports
                .into_iter()
                .map(|(name, value)| (name, self.module_static_value(value)))
                .collect();
            return self.module_namespace(exports);
        }
        if let Some(reason) = crate::Engine::static_module_throw(&module.source) {
            return self.evaluate_static_module_throw(p, reason);
        }
        if crate::Engine::static_module_plan(&module.source, &module.name).is_some_and(|plan| {
            plan.link_plan
                .is_some_and(|link_plan| link_plan.reexports.is_empty())
        }) && let Some(exports) =
            crate::Engine::module_export_names(&module.source, &module.name)
        {
            let mut active = ActiveModuleExports::default();
            active.insert(
                crate::module_identity::normalize(std::path::Path::new(&module.name)),
                Vec::new(),
            );
            let exports = self.evaluate_module_locals(p, module, exports, &mut active)?;
            return self.module_namespace_from_static(exports);
        }
        if module.name.ends_with(".json") {
            return Err(self.type_error(
                p,
                "JSON module import requires a json type attribute".into(),
            ));
        }
        let mut active = ActiveModuleExports::default();
        match self.resolve_static_module_graph(p, module.clone(), &mut active)? {
            StaticModuleGraph::Linked {
                exports,
                incomplete: false,
                ..
            } => self.module_namespace_from_static(exports),
            StaticModuleGraph::Linked {
                incomplete: true, ..
            }
            | StaticModuleGraph::LinkError => {
                self.syntax_error_result(p, "module export could not be resolved unambiguously")
            }
            StaticModuleGraph::Unsupported => {
                Err(self.type_error(p, "dynamic import module evaluation is unsupported".into()))
            }
        }
    }

    pub(super) fn evaluate_program_module_requests(
        &mut self,
        p: &ResidualProgram,
    ) -> Result<(), JsError> {
        if !p.module {
            return Ok(());
        }
        let root = ModuleSource {
            name: p.source_name.clone(),
            source: String::new(),
            bytes: Vec::new(),
        };
        if let Some(namespace) = self.root_module_namespace(p, &root)? {
            self.promise.modules.insert(
                module_cache_key(&p.source_name, "javascript"),
                ModuleRecord::evaluating_main(namespace),
            );
        }
        if p.module_requests.is_empty() {
            return Ok(());
        }
        let mut active = ModuleEvaluationStack::default();
        active.enter(crate::module_identity::normalize(std::path::Path::new(
            &p.source_name,
        )));
        let outer_batch = std::mem::replace(&mut self.deferred_dependency_batch, true);
        let requests =
            self.evaluate_module_requests(p, &p.source_name, &p.module_requests, &mut active);
        self.deferred_dependency_batch = outer_batch;
        requests?;
        self.advance_dynamic_import_jobs(p, false)?;
        let pending_async_evaluation = self.promise.modules.values().any(|record| {
            matches!(
                record.phase(),
                ModulePhase::EvaluatingAsync | ModulePhase::WaitingForDependencies
            )
        });
        if !outer_batch && pending_async_evaluation {
            self.drain_jobs(p)?;
            self.advance_static_module_jobs(p)?;
        }
        let mut linking = ActiveModuleExports::default();
        let imports = self.resolve_module_import_values(
            p,
            p,
            &p.source_name,
            &p.module_imports,
            &mut linking,
        )?;
        self.programs.set_module_imports(ProgramId::MAIN, imports);
        Ok(())
    }

    pub(super) fn advance_dynamic_import_jobs(
        &mut self,
        p: &ResidualProgram,
        allow_evaluation: bool,
    ) -> Result<(), JsError> {
        let jobs = std::mem::take(&mut self.promise.dynamic_import_jobs);
        for job in jobs {
            if let Some(outcome) = self
                .promise
                .modules
                .get(&job.cache_key)
                .map(|record| record.outcome)
                && self.settle_dynamic_import_waiters(p, &job.promises, outcome)?
            {
                continue;
            }
            if !allow_evaluation {
                self.promise.dynamic_import_jobs.push(job);
                continue;
            }
            let mut active = ModuleEvaluationStack::default();
            if let Err(error) = self.evaluate_static_module_source(p, job.module, &mut active) {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                for promise in job.promises {
                    self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                }
                continue;
            }
            let outcome = self
                .promise
                .modules
                .get(&job.cache_key)
                .map(|record| record.outcome)
                .unwrap_or(ModuleOutcome::Pending(ModulePhase::Evaluating));
            if !self.settle_dynamic_import_waiters(p, &job.promises, outcome)?
                && let Some(record) = self.promise.modules.get_mut(&job.cache_key)
            {
                for promise in job.promises {
                    record.add_waiter(promise);
                }
            }
        }
        Ok(())
    }

    fn settle_dynamic_import_waiters(
        &mut self,
        p: &ResidualProgram,
        promises: &[Value],
        outcome: ModuleOutcome,
    ) -> Result<bool, JsError> {
        let (state, value) = match outcome {
            ModuleOutcome::Evaluated(namespace) => (PromiseState::Fulfilled, namespace),
            ModuleOutcome::Errored(reason) => (PromiseState::Rejected, reason),
            ModuleOutcome::Pending(_) | ModuleOutcome::Deferred(_) => return Ok(false),
        };
        for promise in promises {
            self.promise_settle(p, *promise, state, value)?;
        }
        Ok(true)
    }

    pub(super) fn instantiate_main_module(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        if !p.module || self.programs.module_environment(ProgramId::MAIN).is_some() {
            return Ok(());
        }
        let Some(root) = p.functions.first() else {
            return Ok(());
        };
        let mut slots = vec![Value::UNDEFINED; root.local_atoms.len()];
        for atom in &root.lexical_atoms {
            if let Some(slot) = root.local_atoms.iter().position(|local| local == atom) {
                slots[slot] = Value::DELETED;
            }
        }
        let environment = self.heap.alloc(Cell::Environment {
            parent: Value::NULL,
            program: Some(ProgramId::MAIN.raw()),
            root_eval_scope: false,
            function: super::ROOT_FUNCTION_ID,
            slots: slots.into_boxed_slice(),
            dynamic_bindings: Vec::new(),
            with_objects: Vec::new(),
        });
        self.programs
            .set_module_environment(ProgramId::MAIN, environment);

        let hoisted_functions = p
            .module_link_plan
            .as_ref()
            .into_iter()
            .flat_map(|plan| plan.hoisted_functions.iter())
            .filter_map(|(binding, function_name)| {
                let slot = root
                    .local_atoms
                    .iter()
                    .position(|atom| self.atom_name(*atom) == binding)?;
                let function = p
                    .functions
                    .iter()
                    .enumerate()
                    .skip(1)
                    .find(|(_, function)| {
                        function.parent == Some(super::ROOT_FUNCTION_ID)
                            && function
                                .name
                                .is_some_and(|atom| self.atom_name(atom) == function_name)
                    })
                    .map(|(id, _)| id as u32)?;
                Some((slot, function))
            })
            .collect::<Vec<_>>();
        for (slot, function) in hoisted_functions {
            let closure = self.closure(p, function, environment)?;
            if let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(environment)
                && let Some(binding) = slots.get_mut(slot)
            {
                *binding = closure;
            }
        }
        Ok(())
    }

    fn resolve_module_import_values(
        &mut self,
        p: &ResidualProgram,
        bindings: &ResidualProgram,
        referrer: &str,
        imports: &[crate::bytecode::ModuleImportBinding],
        active: &mut ActiveModuleExports,
    ) -> Result<Vec<(u16, crate::vm::program_store::ModuleImport)>, JsError> {
        let Some(root) = bindings.functions.first() else {
            return Ok(Vec::new());
        };
        let mut values = Vec::with_capacity(imports.len());
        for import in imports {
            let module = self
                .host
                .resolve_dynamic_import(referrer, &import.source)
                .map_err(|message| self.type_error(p, message))?
                .ok_or_else(|| self.type_error(p, "module import was not resolved".into()))?;
            let local = self
                .lookup_atom(&import.local)
                .and_then(|atom| {
                    root.local_atoms
                        .iter()
                        .position(|candidate| *candidate == atom)
                })
                .and_then(|slot| u16::try_from(slot).ok())
                .ok_or_else(|| self.type_error(p, "module import slot is unavailable".into()))?;
            if import.phase == crate::bytecode::ModuleRequestPhase::Source {
                values.push((
                    local,
                    crate::vm::program_store::ModuleImport::Value(
                        self.module_source_value(&module),
                    ),
                ));
                continue;
            }
            let module_type = import.module_type.as_deref().unwrap_or("javascript");
            let key = module_cache_key(&module.name, module_type);
            let outcome = self.promise.modules.get(&key).map(|record| record.outcome);
            let deferred_namespace = self
                .promise
                .modules
                .get(&key)
                .and_then(ModuleRecord::deferred_namespace);
            let pending_namespace = self
                .promise
                .modules
                .get(&key)
                .and_then(ModuleRecord::pending_namespace);
            let namespace = if import.phase == crate::bytecode::ModuleRequestPhase::Defer {
                deferred_namespace.or_else(|| match outcome {
                    Some(
                        ModuleOutcome::Evaluated(namespace) | ModuleOutcome::Deferred(namespace),
                    ) => Some(namespace),
                    _ => None,
                })
            } else {
                match outcome {
                    Some(
                        ModuleOutcome::Evaluated(namespace) | ModuleOutcome::Deferred(namespace),
                    ) => Some(namespace),
                    Some(ModuleOutcome::Pending(_)) if pending_namespace.is_some() => {
                        pending_namespace
                    }
                    Some(ModuleOutcome::Pending(_))
                        if self_import_referrer(referrer, &module.name) =>
                    {
                        self.pending_self_import_namespace(p, &module, &key, pending_namespace)?
                    }
                    Some(ModuleOutcome::Errored(reason)) => {
                        return Err(JsError::thrown(reason, "module import failed".into()));
                    }
                    None if self_import_referrer(referrer, &module.name) => {
                        match self.root_module_namespace(p, &module)? {
                            Some(namespace) => Some(namespace),
                            None => self.resolve_self_import_namespace(p, &module, active)?,
                        }
                    }
                    Some(ModuleOutcome::Pending(_)) | None => {
                        return Err(self.type_error(p, "module import was not evaluated".into()));
                    }
                }
            };
            let Some(namespace) = namespace else {
                if let crate::bytecode::ModuleImportName::Named(name) = &import.imported
                    && Self::static_module_declares_export(&module, name) == Some(false)
                {
                    return self
                        .syntax_error_result(p, "module import binding could not be resolved")
                        .map(|_| Vec::new());
                }
                return Err(self.type_error(p, "deferred module namespace is unavailable".into()));
            };
            let named_binding = match (&import.imported, module_type) {
                (crate::bytecode::ModuleImportName::Named(name), "javascript") => {
                    let atom = self.intern_atom(name);
                    self.object_data(namespace)
                        .and_then(|object| object.module_binding(atom))
                }
                _ => None,
            };
            let value = match (&import.phase, &import.imported) {
                (
                    crate::bytecode::ModuleRequestPhase::Defer,
                    crate::bytecode::ModuleImportName::Namespace,
                ) => namespace,
                (_, crate::bytecode::ModuleImportName::Namespace) => namespace,
                (_, crate::bytecode::ModuleImportName::Named(name))
                    if module_type == "json" && name != "default" =>
                {
                    return self
                        .syntax_error_result(p, "JSON modules expose only a default binding")
                        .map(|_| Vec::new());
                }
                (_, crate::bytecode::ModuleImportName::Named(name)) => {
                    let atom = self.intern_atom(name);
                    let is_namespace = self
                        .object_data(namespace)
                        .is_some_and(Object::is_module_namespace);
                    let fallback = self.own_property(namespace, atom);
                    if is_namespace && named_binding.is_none() && fallback.is_none() {
                        return self
                            .syntax_error_result(p, "module import binding could not be resolved")
                            .map(|_| Vec::new());
                    }
                    match (named_binding, fallback) {
                        (Some(_), Some(value)) => value,
                        (Some(_), None) => Value::UNDEFINED,
                        (None, _) => self.get_property(p, namespace, atom)?,
                    }
                }
            };
            let binding = match named_binding {
                Some((program, slot)) => {
                    crate::vm::program_store::ModuleImport::Binding(program, slot, value)
                }
                None => crate::vm::program_store::ModuleImport::Value(value),
            };
            values.push((local, binding));
        }
        Ok(values)
    }

    fn static_module_declares_export(module: &ModuleSource, name: &str) -> Option<bool> {
        crate::Engine::module_export_names(&module.source, &module.name)
            .map(|exports| exports.iter().any(|(_, exported)| exported == name))
    }

    fn resolve_self_import_namespace(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        active: &mut ActiveModuleExports,
    ) -> Result<Option<Value>, JsError> {
        match self.resolve_static_module_graph(p, module.clone(), active)? {
            StaticModuleGraph::Linked {
                exports,
                incomplete: false,
                ..
            } => self.module_namespace_from_static(exports).map(Some),
            StaticModuleGraph::Linked {
                incomplete: true, ..
            }
            | StaticModuleGraph::LinkError => self
                .syntax_error_result(p, "module export could not be resolved unambiguously")
                .map(Some),
            StaticModuleGraph::Unsupported => Ok(None),
        }
    }

    fn pending_self_import_namespace(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        key: &str,
        pending: Option<Value>,
    ) -> Result<Option<Value>, JsError> {
        if pending.is_some() {
            return Ok(pending);
        }
        let Some(namespace) = self.root_module_namespace(p, module)? else {
            return Ok(None);
        };
        if let Some(record) = self.promise.modules.get_mut(key) {
            record.cache_pending_namespace(namespace);
        }
        Ok(Some(namespace))
    }

    fn evaluate_module_requests(
        &mut self,
        p: &ResidualProgram,
        referrer: &str,
        requests: &[crate::bytecode::ModuleRequest],
        active: &mut ModuleEvaluationStack,
    ) -> Result<(), JsError> {
        // Resolve a module's complete request list before evaluating any
        // dependency. Resolution is a host phase; evaluating a dependency
        // while later requests are still unresolved can expose that
        // dependency's link error in place of the host's resolution error.
        let resolved = requests
            .iter()
            .map(|request| {
                self.host
                    .resolve_dynamic_import(referrer, &request.source)
                    .map_err(|message| self.type_error(p, message))?
                    .ok_or_else(|| {
                        self.type_error(p, "static module request was not resolved".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (request, module) in requests.iter().zip(resolved) {
            match request.phase {
                crate::bytecode::ModuleRequestPhase::Evaluation => {
                    let identity =
                        crate::module_identity::normalize(std::path::Path::new(&module.name));
                    if let Some(cycle) = active.cycle_to(&identity) {
                        for member in cycle {
                            let key = module_cache_key(&member.to_string_lossy(), "javascript");
                            if let Some(record) = self.promise.modules.get_mut(&key) {
                                record.set_async_cycle_root(identity.clone());
                            }
                        }
                    }
                    self.evaluate_static_module_request(p, request, module, active)?;
                }
                crate::bytecode::ModuleRequestPhase::Defer => {
                    self.prepare_deferred_module_request(p, module)?;
                }
                crate::bytecode::ModuleRequestPhase::Source => {
                    self.module_source_value(&module);
                }
            }
        }
        Ok(())
    }

    fn prepare_deferred_module_request(
        &mut self,
        p: &ResidualProgram,
        module: ModuleSource,
    ) -> Result<(), JsError> {
        let key = module_cache_key(&module.name, "javascript");
        match self.promise.modules.get(&key).map(|record| record.outcome) {
            Some(ModuleOutcome::Evaluated(_)) => {
                if self
                    .promise
                    .modules
                    .get(&key)
                    .and_then(ModuleRecord::deferred_namespace)
                    .is_none()
                {
                    let namespace = self.deferred_module_namespace(p, &module)?;
                    self.promise
                        .modules
                        .get_mut(&key)
                        .expect("module record checked above")
                        .cache_deferred_namespace(namespace);
                }
                return Ok(());
            }
            Some(ModuleOutcome::Deferred(_)) => return Ok(()),
            Some(ModuleOutcome::Pending(_)) => {
                if self
                    .promise
                    .modules
                    .get(&key)
                    .and_then(ModuleRecord::deferred_namespace)
                    .is_none()
                {
                    let namespace = self.deferred_module_namespace(p, &module)?;
                    self.promise
                        .modules
                        .get_mut(&key)
                        .expect("module record checked above")
                        .cache_deferred_namespace(namespace);
                }
                return Ok(());
            }
            Some(ModuleOutcome::Errored(_)) => {
                if self
                    .promise
                    .modules
                    .get(&key)
                    .and_then(ModuleRecord::deferred_namespace)
                    .is_none()
                {
                    let namespace = self.deferred_module_namespace(p, &module)?;
                    self.promise
                        .modules
                        .get_mut(&key)
                        .expect("module record checked above")
                        .cache_deferred_namespace(namespace);
                }
                return Ok(());
            }
            None => {}
        }
        if crate::Engine::static_module_has_early_error(&module.source) {
            return self
                .syntax_error_result(p, "module source has an early error")
                .map(|_| ());
        }
        let mut seen = FxHashSet::default();
        let mut asynchronous = Vec::new();
        self.gather_async_transitive_dependencies(p, &module, &mut seen, &mut asynchronous)?;
        if !asynchronous.is_empty() {
            let mut active = ModuleEvaluationStack::default();
            let outer_batch = std::mem::replace(&mut self.deferred_dependency_batch, true);
            let launched = (|| {
                for dependency in asynchronous {
                    self.evaluate_static_module_source(p, dependency, &mut active)?;
                }
                Ok::<(), JsError>(())
            })();
            self.deferred_dependency_batch = outer_batch;
            launched?;
            if !outer_batch {
                self.drain_jobs(p)?;
                self.settle_pending_async_modules(p)?;
            }
            let entry_has_tla = crate::Engine::static_module_plan(&module.source, &module.name)
                .is_some_and(|plan| plan.has_top_level_await);
            if entry_has_tla {
                let namespace = self.deferred_module_namespace(p, &module)?;
                if let Some(record) = self.promise.modules.get_mut(&key) {
                    record.cache_deferred_namespace(namespace);
                }
                return Ok(());
            }
        }
        let namespace = self.deferred_module_namespace(p, &module)?;
        match self.promise.modules.get_mut(&key) {
            Some(record) => record.cache_deferred_namespace(namespace),
            None => {
                self.promise
                    .modules
                    .insert(key, ModuleRecord::deferred(namespace));
            }
        }
        Ok(())
    }

    fn evaluate_static_module_request(
        &mut self,
        p: &ResidualProgram,
        request: &crate::bytecode::ModuleRequest,
        module: ModuleSource,
        active: &mut ModuleEvaluationStack,
    ) -> Result<(), JsError> {
        match request.module_type.as_deref().unwrap_or("javascript") {
            "javascript" => self.evaluate_static_module_source(p, module, active),
            module_type @ ("json" | "text" | "bytes") => {
                let key = module_cache_key(&module.name, module_type);
                if self.promise.modules.contains_key(&key) {
                    return Ok(());
                }
                let namespace = self.evaluate_dynamic_module(
                    p,
                    &module,
                    module_type,
                    crate::bytecode::ModuleRequestPhase::Evaluation,
                )?;
                self.promise
                    .modules
                    .insert(key, ModuleRecord::materialized(namespace));
                Ok(())
            }
            _ => Err(self.type_error(p, "unsupported static module type attribute".into())),
        }
    }

    fn evaluate_static_module_source(
        &mut self,
        p: &ResidualProgram,
        module: ModuleSource,
        active: &mut ModuleEvaluationStack,
    ) -> Result<(), JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if active.contains(&identity) {
            return Ok(());
        }
        let cache_key = module_cache_key(&module.name, "javascript");
        let resuming_waiting = matches!(
            self.promise
                .modules
                .get(&cache_key)
                .map(|record| record.outcome),
            Some(ModuleOutcome::Pending(ModulePhase::WaitingForDependencies))
        );
        match self
            .promise
            .modules
            .get(&cache_key)
            .map(|record| record.outcome)
        {
            Some(ModuleOutcome::Evaluated(_)) => return Ok(()),
            Some(ModuleOutcome::Pending(ModulePhase::WaitingForDependencies)) => {
                if self.static_module_has_pending_dependencies(p, &module)? {
                    return Ok(());
                }
                if !self
                    .promise
                    .modules
                    .get_mut(&cache_key)
                    .is_some_and(ModuleRecord::begin_after_dependencies)
                {
                    return Ok(());
                }
            }
            Some(ModuleOutcome::Pending(_)) => return Ok(()),
            Some(ModuleOutcome::Deferred(namespace)) => {
                self.evaluate_deferred_module_namespace(p, namespace)?;
                return Ok(());
            }
            Some(ModuleOutcome::Errored(reason)) => {
                return Err(JsError::thrown(
                    reason,
                    "static module evaluation failed".into(),
                ));
            }
            None => {}
        }
        if crate::Engine::static_module_has_early_error(&module.source) {
            return self
                .syntax_error_result(p, "module source has an early error")
                .map(|_| ());
        }
        let Some(plan) = crate::Engine::static_module_plan(&module.source, &module.name) else {
            return Err(self.type_error(p, "static module metadata is unavailable".into()));
        };
        if !resuming_waiting {
            self.promise
                .modules
                .insert(cache_key.clone(), ModuleRecord::evaluating_static());
        }
        active.enter(identity.clone());
        let result = self.evaluate_static_module_body(p, &module, &plan, active);
        active.leave(&identity);
        match result {
            Ok(Some(namespace)) => self.settle_static_module(p, &cache_key, Ok(namespace)),
            Ok(None) => {
                let waiting = self
                    .promise
                    .modules
                    .get_mut(&cache_key)
                    .is_some_and(ModuleRecord::wait_for_dependencies);
                if waiting {
                    self.promise.waiting_static_modules.push(module);
                }
                Ok(())
            }
            Err(error) => self.settle_static_module(p, &cache_key, Err(error)),
        }
    }

    fn evaluate_static_module_body(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        plan: &crate::compile::StaticModulePlan,
        active: &mut ModuleEvaluationStack,
    ) -> Result<Option<Value>, JsError> {
        self.evaluate_module_requests(p, &module.name, &plan.requests, active)?;
        if self.static_module_has_pending_dependencies(p, module)? {
            return Ok(None);
        }
        self.evaluate_javascript_module_body(p, module).map(Some)
    }

    fn settle_static_module(
        &mut self,
        p: &ResidualProgram,
        cache_key: &str,
        result: Result<Value, JsError>,
    ) -> Result<(), JsError> {
        let (state, value) = match result {
            Ok(namespace) => (PromiseState::Fulfilled, namespace),
            Err(error) => (
                PromiseState::Rejected,
                error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.to_string()))),
            ),
        };
        if state == PromiseState::Fulfilled {
            self.refresh_static_module_bindings(p, value)?;
        }
        if state == PromiseState::Fulfilled
            && self
                .promise
                .modules
                .get(cache_key)
                .and_then(ModuleRecord::evaluation_promise)
                .is_some_and(|promise| {
                    self.promise
                        .records
                        .get(&promise)
                        .is_some_and(|record| record.state == PromiseState::Pending)
                })
        {
            let Some(record) = self.promise.modules.get_mut(cache_key) else {
                return Err(
                    self.type_error(p, "module record disappeared during evaluation".into())
                );
            };
            record.begin_async_evaluation(value);
            self.promise
                .async_module_order
                .push_back(cache_key.to_owned());
            return Ok(());
        }
        let deferred_namespace = self
            .promise
            .modules
            .get(cache_key)
            .and_then(ModuleRecord::deferred_namespace);
        if state == PromiseState::Fulfilled
            && let Some(namespace) = deferred_namespace
        {
            self.copy_module_namespace(value, namespace, p)?;
        }
        let Some(record) = self.promise.modules.get_mut(cache_key) else {
            return Err(self.type_error(p, "module record disappeared during evaluation".into()));
        };
        let waiters = match state {
            PromiseState::Fulfilled => record.evaluate(value),
            PromiseState::Rejected => record.fail(value),
            PromiseState::Pending => None,
        };
        let Some(waiters) = waiters else {
            return Err(self.type_error(p, "module record was not evaluating".into()));
        };
        let rejection_is_observed = state != PromiseState::Rejected
            || !waiters.is_empty()
            || self.waiting_static_parent(p, cache_key)?;
        for waiter in waiters {
            self.promise_settle(p, waiter, state, value)?;
        }
        if state == PromiseState::Rejected && !rejection_is_observed {
            return Err(JsError::thrown(
                value,
                "static module evaluation failed".into(),
            ));
        }
        Ok(())
    }

    fn waiting_static_parent(
        &mut self,
        p: &ResidualProgram,
        dependency_key: &str,
    ) -> Result<bool, JsError> {
        let waiting = self.promise.waiting_static_modules.clone();
        for parent in waiting {
            let Some(plan) = crate::Engine::static_module_plan(&parent.source, &parent.name) else {
                return Err(self.type_error(p, "static module metadata is unavailable".into()));
            };
            for request in plan.requests.iter().filter(|request| {
                request.phase == crate::bytecode::ModuleRequestPhase::Evaluation
                    && request.module_type.as_deref().unwrap_or("javascript") == "javascript"
            }) {
                let dependency = self
                    .host
                    .resolve_dynamic_import(&parent.name, &request.source)
                    .map_err(|message| self.type_error(p, message))?
                    .ok_or_else(|| {
                        self.type_error(p, "static module request was not resolved".into())
                    })?;
                if module_cache_key(&dependency.name, "javascript") == dependency_key {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn refresh_static_module_bindings(
        &mut self,
        p: &ResidualProgram,
        namespace: Value,
    ) -> Result<(), JsError> {
        let bindings = self
            .object_data(namespace)
            .map(|object| object.module_bindings.clone())
            .unwrap_or_default();
        for (atom, program, slot) in bindings {
            let Some(environment) = self.programs.module_environment(program) else {
                continue;
            };
            let Some(Cell::Environment { slots, .. }) = self.heap.get(environment) else {
                return Err(self.type_error(p, "module environment is unavailable".into()));
            };
            let value = slots
                .get(usize::from(slot))
                .copied()
                .ok_or_else(|| self.type_error(p, "module export slot is unavailable".into()))?;
            let property = self
                .object_data(namespace)
                .and_then(|object| self.shape_slot(object.shape(), atom))
                .ok_or_else(|| {
                    self.type_error(p, "module export property is unavailable".into())
                })?;
            self.heap.property_set(namespace, property, value);
        }
        Ok(())
    }

    pub(super) fn advance_static_module_jobs(
        &mut self,
        p: &ResidualProgram,
    ) -> Result<(), JsError> {
        self.settle_pending_async_modules(p)?;
        let waiting = std::mem::take(&mut self.promise.waiting_static_modules);
        for module in waiting {
            if self.static_module_has_pending_dependencies(p, &module)? {
                self.promise.waiting_static_modules.push(module);
                continue;
            }
            let cache_key = module_cache_key(&module.name, "javascript");
            let Some(record) = self.promise.modules.get(&cache_key) else {
                continue;
            };
            if record.phase() != ModulePhase::WaitingForDependencies {
                continue;
            }
            let mut active = ModuleEvaluationStack::default();
            let outer_batch = std::mem::replace(&mut self.deferred_dependency_batch, true);
            let result = self.evaluate_static_module_source(p, module, &mut active);
            self.deferred_dependency_batch = outer_batch;
            result?;
        }
        Ok(())
    }

    fn settle_pending_async_modules(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let pending = self
            .promise
            .async_module_order
            .iter()
            .filter_map(|key| {
                let module = self.promise.modules.get(key)?;
                (module.phase() == ModulePhase::EvaluatingAsync).then(|| {
                    Some((
                        key.clone(),
                        module.evaluation_promise()?,
                        module.pending_namespace()?,
                    ))
                })?
            })
            .collect::<Vec<_>>();
        for (key, promise, namespace) in pending {
            let Some(record) = self.promise.records.get(&promise).cloned() else {
                continue;
            };
            match record.state {
                PromiseState::Pending => {}
                PromiseState::Fulfilled => self.settle_static_module(p, &key, Ok(namespace))?,
                PromiseState::Rejected => self.settle_static_module(
                    p,
                    &key,
                    Err(JsError::thrown(
                        record.result,
                        "module evaluation rejected".into(),
                    )),
                )?,
            }
        }
        self.promise.async_module_order.retain(|key| {
            self.promise
                .modules
                .get(key)
                .is_some_and(|module| module.phase() == ModulePhase::EvaluatingAsync)
        });
        Ok(())
    }

    fn static_module_has_pending_dependencies(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<bool, JsError> {
        let Some(plan) = crate::Engine::static_module_plan(&module.source, &module.name) else {
            return Err(self.type_error(p, "static module metadata is unavailable".into()));
        };
        for request in plan
            .requests
            .iter()
            .filter(|request| request.phase == crate::bytecode::ModuleRequestPhase::Evaluation)
        {
            if request.module_type.as_deref().unwrap_or("javascript") != "javascript" {
                continue;
            }
            let dependency = self
                .host
                .resolve_dynamic_import(&module.name, &request.source)
                .map_err(|message| self.type_error(p, message))?
                .ok_or_else(|| {
                    self.type_error(p, "static module request was not resolved".into())
                })?;
            let dependency_key = module_cache_key(&dependency.name, "javascript");
            let dependency_record = self.promise.modules.get(&dependency_key);
            let phase = dependency_record.map(ModuleRecord::phase);
            if matches!(
                phase,
                Some(ModulePhase::EvaluatingAsync | ModulePhase::WaitingForDependencies)
            ) {
                return Ok(true);
            }
            let cycle_root = dependency_record.and_then(ModuleRecord::async_cycle_root);
            let module_identity =
                crate::module_identity::normalize(std::path::Path::new(&module.name));
            let cycle_root_phase = cycle_root
                .filter(|root| **root != module_identity)
                .and_then(|root| {
                    self.promise
                        .modules
                        .get(&module_cache_key(&root.to_string_lossy(), "javascript"))
                })
                .map(ModuleRecord::phase);
            if matches!(
                cycle_root_phase,
                Some(ModulePhase::EvaluatingAsync | ModulePhase::WaitingForDependencies)
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn evaluate_module_locals(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        exports: Vec<(String, String)>,
        active: &mut ActiveModuleExports,
    ) -> Result<Vec<(String, StaticModuleValue)>, JsError> {
        let atom_prefix = (0..self.atom_text.len() + self.dynamic_atoms.len())
            .map(|atom| self.atom_name(atom as u32).to_owned())
            .collect::<Vec<_>>();
        let residual = crate::Engine::specialize_module_unspecialized_with_atom_prefix(
            &module.source,
            &module.name,
            &atom_prefix,
        )
        .map_err(|diagnostics| {
            self.type_error(
                p,
                format!("dynamic module compilation failed: {diagnostics:?}"),
            )
        })?;
        let Some(program_id) = self.store_module_program(residual) else {
            return Err(self.type_error(p, "module program store is full".into()));
        };
        let residual = self
            .programs
            .get(program_id)
            .ok_or_else(|| self.type_error(p, "module program is unavailable".into()))?;
        let imports = self.resolve_module_import_values(
            p,
            &residual,
            &module.name,
            &residual.module_imports,
            active,
        )?;
        let mut source_import_values = FxHashMap::default();
        for import in residual
            .module_imports
            .iter()
            .filter(|import| import.phase == crate::bytecode::ModuleRequestPhase::Source)
        {
            let atom = residual
                .atoms
                .iter()
                .position(|name| name == &import.local)
                .ok_or_else(|| self.type_error(p, "source import binding is unavailable".into()))?
                as u32;
            let slot = residual.functions[0]
                .local_atoms
                .iter()
                .position(|candidate| *candidate == atom)
                .and_then(|slot| u16::try_from(slot).ok())
                .ok_or_else(|| self.type_error(p, "source import slot is unavailable".into()))?;
            let value = imports
                .iter()
                .find_map(
                    |(import_slot, import)| match (*import_slot == slot, import) {
                        (true, crate::vm::program_store::ModuleImport::Value(value)) => {
                            Some(*value)
                        }
                        (true, crate::vm::program_store::ModuleImport::Binding(_, _, _)) => None,
                        (false, _) => None,
                    },
                )
                .ok_or_else(|| self.type_error(p, "source import value is unavailable".into()))?;
            source_import_values.insert(import.local.clone(), value);
        }
        self.programs.set_module_imports(program_id, imports);
        let active_program = std::mem::replace(&mut self.active_program, program_id);
        let specialized = std::mem::replace(&mut self.specialized, false);
        let evaluated = (|| {
            let closure = self.closure(&residual, 0, Value::NULL)?;
            let evaluation = self.call_value(&residual, closure, Value::UNDEFINED, &[])?;
            if residual
                .functions
                .first()
                .is_some_and(|function| function.is_async)
            {
                let pending = self
                    .promise
                    .records
                    .get(&evaluation)
                    .is_some_and(|record| record.state == PromiseState::Pending);
                if pending && self.deferred_dependency_batch {
                    let key = module_cache_key(&module.name, "javascript");
                    let Some(module_record) = self.promise.modules.get_mut(&key) else {
                        return Err(self.type_error(
                            p,
                            "module record disappeared during asynchronous evaluation".into(),
                        ));
                    };
                    module_record.track_evaluation_promise(evaluation);
                } else if pending {
                    self.drain_jobs_until_promise(&residual, evaluation)?;
                }
                let Some(record) = self.promise.records.get(&evaluation).cloned() else {
                    return Err(
                        self.type_error(p, "module evaluation promise is unavailable".into())
                    );
                };
                match record.state {
                    PromiseState::Fulfilled => {}
                    PromiseState::Rejected => {
                        return Err(JsError::thrown(
                            record.result,
                            "module evaluation rejected".into(),
                        ));
                    }
                    PromiseState::Pending if self.deferred_dependency_batch => {}
                    PromiseState::Pending => {
                        return Err(self.type_error(
                            p,
                            "top-level await did not complete during module evaluation".into(),
                        ));
                    }
                }
            }
            let mut values = Vec::with_capacity(exports.len());
            for (local, exported) in exports {
                let atom = residual
                    .atoms
                    .iter()
                    .position(|name| name == local)
                    .ok_or_else(|| {
                        self.type_error(p, "module export binding is unavailable".into())
                    })? as u32;
                let hoisted_function = residual
                    .functions
                    .iter()
                    .enumerate()
                    .find(|(_, function)| {
                        function.parent == Some(0)
                            && function
                                .name
                                .is_some_and(|name| residual.atoms[name as usize] == local)
                    })
                    .and_then(|(id, _)| {
                        self.function_values
                            .get(&(program_id, id as u32))
                            .and_then(|values| values.last())
                            .map(|(_, value)| *value)
                    });
                let slot = residual.functions[0]
                    .local_atoms
                    .iter()
                    .position(|candidate| *candidate == atom)
                    .and_then(|slot| u16::try_from(slot).ok())
                    .ok_or_else(|| {
                        self.type_error(p, "module export slot is unavailable".into())
                    })?;
                let value = match hoisted_function {
                    Some(value) => value,
                    None => {
                        let environment =
                            self.programs
                                .module_environment(program_id)
                                .ok_or_else(|| {
                                    self.type_error(p, "module environment is unavailable".into())
                                })?;
                        let Some(Cell::Environment { slots, .. }) = self.heap.get(environment)
                        else {
                            return Err(
                                self.type_error(p, "module environment is unavailable".into())
                            );
                        };
                        slots.get(slot as usize).copied().ok_or_else(|| {
                            self.type_error(p, "module export slot is unavailable".into())
                        })?
                    }
                };
                let binding = match source_import_values.get(&local) {
                    Some(value) => StaticModuleValue::ModuleSource(*value),
                    None => StaticModuleValue::Binding {
                        program: program_id,
                        slot,
                        value,
                    },
                };
                values.push((exported, binding));
            }
            Ok(values)
        })();
        self.active_program = active_program;
        self.specialized = specialized;
        evaluated
    }

    fn evaluate_static_module_throw(
        &mut self,
        p: &ResidualProgram,
        thrown: crate::compile::StaticModuleThrow,
    ) -> Result<Value, JsError> {
        let reason = match thrown {
            crate::compile::StaticModuleThrow::Value(value) => self.module_static_value(value),
            crate::compile::StaticModuleThrow::Error { name, message } => {
                let kind = match name.as_str() {
                    "EvalError" => Native::EvalError,
                    "RangeError" => Native::RangeError,
                    "ReferenceError" => Native::ReferenceError,
                    "SyntaxError" => Native::SyntaxError,
                    "TypeError" => Native::TypeError,
                    "URIError" => Native::URIError,
                    _ => Native::Error,
                };
                let arguments = message
                    .map(|message| self.module_static_value(message))
                    .into_iter()
                    .collect::<Vec<_>>();
                self.construct_error_native(p, kind, &arguments)?
            }
        };
        Err(JsError::thrown(reason, "module evaluation threw".into()))
    }

    fn ensure_static_module_node(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        nodes: &mut StaticModuleNodes,
    ) -> Result<bool, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if nodes.contains_key(&identity) {
            return Ok(true);
        }
        if let Some(exports) = crate::Engine::static_module_exports(&module.source, &module.name) {
            let exports = exports
                .into_iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        StaticModuleValue::Constant {
                            module: module.name.clone(),
                            export: name,
                            value,
                        },
                    )
                })
                .collect();
            nodes.insert(identity, StaticModuleNode::Direct(exports));
            return Ok(true);
        }
        let Some(plan) = crate::Engine::static_module_plan(&module.source, &module.name) else {
            return Ok(false);
        };
        let Some(link_plan) = plan.link_plan else {
            return Ok(false);
        };
        let atom_prefix = (0..self.atom_text.len() + self.dynamic_atoms.len())
            .map(|atom| self.atom_name(atom as u32).to_owned())
            .collect::<Vec<_>>();
        let residual = crate::Engine::specialize_module_unspecialized_with_atom_prefix(
            &module.source,
            &module.name,
            &atom_prefix,
        )
        .map_err(|diagnostics| {
            self.type_error(
                p,
                format!("dynamic module compilation failed: {diagnostics:?}"),
            )
        })?;
        if !residual.module_imports.is_empty() {
            return Ok(false);
        }
        let locals = if link_plan.locals.is_empty() {
            Vec::new()
        } else {
            self.evaluate_module_locals(
                p,
                module,
                link_plan.locals,
                &mut ActiveModuleExports::default(),
            )?
        };
        nodes.insert(
            identity,
            StaticModuleNode::Planned {
                locals,
                reexports: link_plan.reexports,
            },
        );
        Ok(true)
    }

    fn static_module_export_names(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        nodes: &mut StaticModuleNodes,
        visited: &mut FxHashSet<std::path::PathBuf>,
    ) -> Result<Option<Vec<String>>, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if !visited.insert(identity.clone()) {
            return Ok(Some(Vec::new()));
        }
        if !self.ensure_static_module_node(p, module, nodes)? {
            return Ok(None);
        }
        let Some(node) = nodes.get(&identity).cloned() else {
            return Ok(None);
        };
        let mut names = Vec::new();
        match node {
            StaticModuleNode::Direct(exports) => {
                names.extend(exports.into_iter().map(|(name, _)| name));
            }
            StaticModuleNode::Planned { locals, reexports } => {
                names.extend(locals.into_iter().map(|(name, _)| name));
                for reexport in reexports {
                    match reexport {
                        crate::compile::StaticModuleReexport::Named { exported, .. }
                        | crate::compile::StaticModuleReexport::Namespace { exported, .. } => {
                            names.push(exported);
                        }
                        crate::compile::StaticModuleReexport::Star { source } => {
                            let Some(dependency) =
                                self.static_module_dependency(p, &module.name, &source)?
                            else {
                                return Ok(None);
                            };
                            let Some(dependency_names) =
                                self.static_module_export_names(p, &dependency, nodes, visited)?
                            else {
                                return Ok(None);
                            };
                            names.extend(dependency_names);
                        }
                    }
                }
            }
        }
        names.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
        names.dedup();
        Ok(Some(names))
    }

    fn static_module_dependency(
        &mut self,
        p: &ResidualProgram,
        referrer: &str,
        source: &str,
    ) -> Result<Option<ModuleSource>, JsError> {
        self.host
            .resolve_dynamic_import(referrer, source)
            .map_err(|message| self.type_error(p, message))
    }

    fn resolve_static_module_export(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        name: &str,
        nodes: &mut StaticModuleNodes,
        resolve_set: &mut StaticModuleResolveSet,
    ) -> Result<StaticModuleResolution, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if !resolve_set.insert((identity.clone(), name.to_owned())) {
            return Ok(StaticModuleResolution::Missing);
        }
        if !self.ensure_static_module_node(p, module, nodes)? {
            return Ok(StaticModuleResolution::Unsupported);
        }
        let Some(node) = nodes.get(&identity).cloned() else {
            return Ok(StaticModuleResolution::Unsupported);
        };
        let (locals, reexports) = match node {
            StaticModuleNode::Direct(exports) => (exports, Vec::new()),
            StaticModuleNode::Planned { locals, reexports } => (locals, reexports),
        };
        if let Some((_, value)) = locals.into_iter().find(|(export, _)| export == name) {
            return Ok(StaticModuleResolution::Binding(value));
        }
        for reexport in &reexports {
            if let crate::compile::StaticModuleReexport::Named {
                source,
                imported,
                exported,
            } = reexport
                && exported == name
            {
                let Some(dependency) = self.static_module_dependency(p, &module.name, source)?
                else {
                    return Ok(StaticModuleResolution::Missing);
                };
                return self.resolve_static_module_export(
                    p,
                    &dependency,
                    imported,
                    nodes,
                    resolve_set,
                );
            }
            if let crate::compile::StaticModuleReexport::Namespace { source, exported } = reexport
                && exported == name
            {
                let Some(dependency) = self.static_module_dependency(p, &module.name, source)?
                else {
                    return Ok(StaticModuleResolution::Missing);
                };
                let Some(names) = self.static_module_export_names(
                    p,
                    &dependency,
                    nodes,
                    &mut FxHashSet::default(),
                )?
                else {
                    return Ok(StaticModuleResolution::Unsupported);
                };
                let mut exports = Vec::new();
                for name in names {
                    match self.resolve_static_module_export(
                        p,
                        &dependency,
                        &name,
                        nodes,
                        &mut StaticModuleResolveSet::default(),
                    )? {
                        StaticModuleResolution::Binding(value) => exports.push((name, value)),
                        StaticModuleResolution::Missing | StaticModuleResolution::Ambiguous => {}
                        StaticModuleResolution::Unsupported => {
                            return Ok(StaticModuleResolution::Unsupported);
                        }
                    }
                }
                return Ok(StaticModuleResolution::Binding(
                    StaticModuleValue::Namespace {
                        name: dependency.name,
                        exports,
                    },
                ));
            }
        }
        let mut star_resolution: Option<StaticModuleValue> = None;
        for reexport in reexports {
            let crate::compile::StaticModuleReexport::Star { source } = reexport else {
                continue;
            };
            if name == "default" {
                continue;
            }
            let Some(dependency) = self.static_module_dependency(p, &module.name, &source)? else {
                return Ok(StaticModuleResolution::Missing);
            };
            let mut branch = resolve_set.clone();
            match self.resolve_static_module_export(p, &dependency, name, nodes, &mut branch)? {
                StaticModuleResolution::Binding(value) => match &star_resolution {
                    Some(existing) if !same_static_module_binding(existing, &value) => {
                        return Ok(StaticModuleResolution::Ambiguous);
                    }
                    None => star_resolution = Some(value),
                    _ => {}
                },
                StaticModuleResolution::Ambiguous => {
                    return Ok(StaticModuleResolution::Ambiguous);
                }
                StaticModuleResolution::Missing => {}
                StaticModuleResolution::Unsupported => {
                    return Ok(StaticModuleResolution::Unsupported);
                }
            }
        }
        Ok(star_resolution.map_or(
            StaticModuleResolution::Missing,
            StaticModuleResolution::Binding,
        ))
    }

    fn validate_static_module_links(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        nodes: &mut StaticModuleNodes,
        visited: &mut FxHashSet<std::path::PathBuf>,
    ) -> Result<Option<bool>, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if !visited.insert(identity.clone()) {
            return Ok(Some(true));
        }
        if !self.ensure_static_module_node(p, module, nodes)? {
            return Ok(None);
        }
        let Some(StaticModuleNode::Planned { reexports, .. }) = nodes.get(&identity).cloned()
        else {
            return Ok(Some(true));
        };
        for reexport in reexports {
            let (source, named) = match reexport {
                crate::compile::StaticModuleReexport::Named {
                    source, imported, ..
                } => (source, Some(imported)),
                crate::compile::StaticModuleReexport::Star { source }
                | crate::compile::StaticModuleReexport::Namespace { source, .. } => (source, None),
            };
            let Some(dependency) = self.static_module_dependency(p, &module.name, &source)? else {
                return Ok(Some(false));
            };
            if let Some(imported) = named {
                match self.resolve_static_module_export(
                    p,
                    &dependency,
                    &imported,
                    nodes,
                    &mut StaticModuleResolveSet::default(),
                )? {
                    StaticModuleResolution::Binding(_) => {}
                    StaticModuleResolution::Missing | StaticModuleResolution::Ambiguous => {
                        return Ok(Some(false));
                    }
                    StaticModuleResolution::Unsupported => return Ok(None),
                }
            }
            let Some(valid) = self.validate_static_module_links(p, &dependency, nodes, visited)?
            else {
                return Ok(None);
            };
            if !valid {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    }

    fn resolve_static_module_plan_graph(
        &mut self,
        p: &ResidualProgram,
        module: ModuleSource,
    ) -> Result<Option<StaticModuleGraph>, JsError> {
        let mut nodes = StaticModuleNodes::default();
        let Some(names) =
            self.static_module_export_names(p, &module, &mut nodes, &mut FxHashSet::default())?
        else {
            return Ok(None);
        };
        let Some(valid) =
            self.validate_static_module_links(p, &module, &mut nodes, &mut FxHashSet::default())?
        else {
            return Ok(None);
        };
        if !valid {
            return Ok(Some(StaticModuleGraph::LinkError));
        }
        let mut exports = Vec::new();
        for name in names {
            match self.resolve_static_module_export(
                p,
                &module,
                &name,
                &mut nodes,
                &mut StaticModuleResolveSet::default(),
            )? {
                StaticModuleResolution::Binding(value) => exports.push((name, value)),
                StaticModuleResolution::Missing | StaticModuleResolution::Ambiguous => {}
                StaticModuleResolution::Unsupported => return Ok(None),
            }
        }
        Ok(Some(StaticModuleGraph::Linked {
            name: module.name,
            exports,
            incomplete: false,
        }))
    }

    fn resolve_static_module_graph(
        &mut self,
        p: &ResidualProgram,
        module: ModuleSource,
        active: &mut ActiveModuleExports,
    ) -> Result<StaticModuleGraph, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if let Some(exports) = active.get(&identity) {
            return Ok(StaticModuleGraph::Linked {
                name: module.name.clone(),
                exports: exports.clone(),
                incomplete: true,
            });
        }
        if crate::Engine::static_module_plan(&module.source, &module.name).is_some_and(|plan| {
            plan.link_plan
                .is_some_and(|link_plan| !link_plan.reexports.is_empty())
        }) && let Some(graph) = self.resolve_static_module_plan_graph(p, module.clone())?
        {
            return Ok(graph);
        }
        if p.module
            && self.active_program == ProgramId::MAIN
            && crate::module_identity::same_name(&module.name, &p.source_name)
        {
            let key = module_cache_key(&module.name, "javascript");
            let namespace = self
                .promise
                .modules
                .get(&key)
                .and_then(ModuleRecord::pending_namespace);
            if let Some(namespace) = namespace
                && let Some(exports) = self.cached_static_exports(namespace)
            {
                return Ok(StaticModuleGraph::Linked {
                    name: module.name.clone(),
                    exports,
                    incomplete: false,
                });
            }
        }
        if let Some(exports) = crate::Engine::static_module_exports(&module.source, &module.name) {
            return Ok(StaticModuleGraph::Linked {
                name: module.name.clone(),
                exports: exports
                    .into_iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            StaticModuleValue::Constant {
                                module: module.name.clone(),
                                export: name,
                                value,
                            },
                        )
                    })
                    .collect(),
                incomplete: false,
            });
        }
        if let Some(plan) = crate::Engine::static_module_plan(&module.source, &module.name)
            && plan
                .link_plan
                .as_ref()
                .is_some_and(|link_plan| !link_plan.reexports.is_empty())
        {
            active.insert(identity.clone(), Vec::new());
            let result = self.resolve_static_module_plan(p, &module, plan, active);
            active.remove(&identity);
            return result;
        }
        let key = module_cache_key(&module.name, "javascript");
        let namespace = self
            .promise
            .modules
            .get(&key)
            .and_then(|record| match record.outcome {
                ModuleOutcome::Evaluated(namespace) | ModuleOutcome::Deferred(namespace) => {
                    Some(namespace)
                }
                ModuleOutcome::Pending(_) => record.pending_namespace(),
                ModuleOutcome::Errored(_) => None,
            });
        if let Some(namespace) = namespace
            && let Some(exports) = self.cached_static_exports(namespace)
            && !exports.is_empty()
        {
            return Ok(StaticModuleGraph::Linked {
                name: module.name,
                exports,
                incomplete: false,
            });
        }
        Ok(StaticModuleGraph::Unsupported)
    }

    fn resolve_static_module_plan(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        plan: crate::compile::StaticModulePlan,
        active: &mut ActiveModuleExports,
    ) -> Result<StaticModuleGraph, JsError> {
        let Some(link_plan) = plan.link_plan else {
            return Ok(StaticModuleGraph::Unsupported);
        };
        let locals = if link_plan.locals.is_empty() {
            Vec::new()
        } else {
            self.evaluate_module_locals(p, module, link_plan.locals, active)?
        };
        self.resolve_static_module_links(p, module, locals, link_plan.reexports, active)
    }

    fn resolve_static_module_links(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        locals: Vec<(String, StaticModuleValue)>,
        reexports: Vec<crate::compile::StaticModuleReexport>,
        active: &mut ActiveModuleExports,
    ) -> Result<StaticModuleGraph, JsError> {
        let mut links = StaticModuleLinks::default();
        if links.add_locals(locals.clone()) != StaticModuleAdd::Added {
            return Ok(StaticModuleGraph::LinkError);
        }
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        active.insert(identity.clone(), links.partial_exports());
        let mut pending = reexports;
        loop {
            let mut remaining = Vec::new();
            let mut changed = false;
            for reexport in pending {
                let source = match &reexport {
                    crate::compile::StaticModuleReexport::Named { source, .. }
                    | crate::compile::StaticModuleReexport::Star { source }
                    | crate::compile::StaticModuleReexport::Namespace { source, .. } => source,
                };
                let dependency = self
                    .host
                    .resolve_dynamic_import(&module.name, source)
                    .map_err(|message| self.type_error(p, message))?;
                let Some(dependency) = dependency else {
                    return Ok(StaticModuleGraph::LinkError);
                };
                let StaticModuleGraph::Linked {
                    name: dependency_name,
                    exports: dependency_exports,
                    incomplete,
                } = self.resolve_static_module_graph(p, dependency, active)?
                else {
                    return Ok(StaticModuleGraph::Unsupported);
                };
                match links.add(reexport.clone(), dependency_name, dependency_exports) {
                    StaticModuleAdd::Added => changed = true,
                    StaticModuleAdd::Missing if incomplete => {
                        remaining.push(reexport);
                        continue;
                    }
                    StaticModuleAdd::Missing => return Ok(StaticModuleGraph::LinkError),
                    StaticModuleAdd::Conflict => return Ok(StaticModuleGraph::LinkError),
                }
                active.insert(identity.clone(), links.partial_exports());
            }
            if remaining.is_empty() {
                active.remove(&identity);
                return Ok(links.finish(module.name.clone(), false));
            }
            if !changed {
                let result = links.finish(module.name.clone(), true);
                active.remove(&identity);
                return Ok(result);
            }
            pending = remaining;
        }
    }

    fn module_namespace_from_static(
        &mut self,
        exports: Vec<(String, StaticModuleValue)>,
    ) -> Result<Value, JsError> {
        let mut values = Vec::with_capacity(exports.len());
        let mut bindings = Vec::new();
        for (name, value) in exports {
            let value = match value {
                StaticModuleValue::Constant { value, .. } => self.module_static_value(value),
                StaticModuleValue::Cached(value) => value,
                StaticModuleValue::ModuleSource(value) => value,
                StaticModuleValue::Binding {
                    program,
                    slot,
                    value,
                } => {
                    let atom = self.intern_atom(&name);
                    bindings.push((atom, program, slot));
                    value
                }
                StaticModuleValue::Namespace { name, exports } => {
                    self.cached_static_namespace(name, exports)?
                }
            };
            values.push((name, value));
        }
        let namespace = self.module_namespace(values)?;
        if let Some(object) = self.object_data_mut(namespace) {
            object.module_bindings = bindings;
        }
        Ok(namespace)
    }

    fn cached_static_exports(&self, namespace: Value) -> Option<Vec<(String, StaticModuleValue)>> {
        let object = self.object_data(namespace)?;
        let bindings = object
            .module_bindings
            .iter()
            .map(|(atom, program, slot)| (*atom, (*program, *slot)))
            .collect::<FxHashMap<_, _>>();
        Some(
            self.shapes
                .get(object.shape() as usize)?
                .keys
                .iter()
                .filter_map(|key| {
                    let crate::vm::property_key::PropertyKey::String(atom) = key else {
                        return None;
                    };
                    let value = self.own_property(namespace, *atom)?;
                    let export = match bindings.get(atom) {
                        Some((program, slot)) => StaticModuleValue::Binding {
                            program: *program,
                            slot: *slot,
                            value,
                        },
                        None => StaticModuleValue::Cached(value),
                    };
                    Some((self.atom_name(*atom).to_owned(), export))
                })
                .collect::<Vec<_>>(),
        )
    }

    fn cached_static_namespace(
        &mut self,
        name: String,
        exports: Vec<(String, StaticModuleValue)>,
    ) -> Result<Value, JsError> {
        let key = module_cache_key(&name, "javascript");
        if let Some(ModuleOutcome::Evaluated(namespace)) =
            self.promise.modules.get(&key).map(|record| record.outcome)
        {
            return Ok(namespace);
        }
        let namespace = self.module_namespace_from_static(exports)?;
        self.promise
            .modules
            .insert(key, ModuleRecord::materialized(namespace));
        Ok(namespace)
    }

    fn validate_dynamic_import_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<Option<String>, JsError> {
        if options.is_undefined() {
            return Ok(None);
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "dynamic import options must be an object".into()));
        }
        let with_atom = self.intern_atom("with");
        let mut attributes = self.get_property(p, options, with_atom)?;
        if attributes.is_undefined() {
            let assert_atom = self.intern_atom("assert");
            attributes = self.get_property(p, options, assert_atom)?;
        }
        if attributes.is_undefined() {
            return Ok(None);
        }
        if !self.is_object_like(attributes) {
            return Err(self.type_error(p, "dynamic import attributes must be an object".into()));
        }
        let keys = self.object_own_keys(p, attributes)?;
        let Some(Cell::Array { elements, .. }) = self.heap.get(keys) else {
            return Err(JsError(
                "dynamic import own keys result is not an array".into(),
            ));
        };
        let keys = elements.as_ref().clone();
        let mut module_type = None;
        for key in keys {
            let key_name = match self.heap.get(key) {
                Some(Cell::String(value)) => Some(value.host_string().to_owned()),
                Some(Cell::Symbol(_)) => continue,
                _ => return Err(JsError("invalid import attribute key".into())),
            };
            let descriptor = self.object_get_own_property_descriptor(p, &[attributes, key])?;
            if descriptor.is_undefined() {
                continue;
            }
            let enumerable_atom = self.intern_atom("enumerable");
            let enumerable = self.get_property(p, descriptor, enumerable_atom)?;
            if self.truthy(enumerable) {
                let value = self.get_index(p, attributes, key)?;
                if !matches!(self.heap.get(value), Some(Cell::String(_))) {
                    return Err(self
                        .type_error(p, "dynamic import attribute values must be strings".into()));
                }
                if key_name.as_deref() == Some("type")
                    && let Some(Cell::String(value)) = self.heap.get(value)
                {
                    module_type = Some(value.host_string().to_owned());
                }
            }
        }
        Ok(module_type)
    }

    fn module_namespace_default(&mut self, value: Value) -> Result<Value, JsError> {
        self.module_namespace(vec![("default".into(), value)])
    }

    fn module_namespace(&mut self, exports: Vec<(String, Value)>) -> Result<Value, JsError> {
        self.module_namespace_with_tag(exports, "Module")
    }

    fn module_namespace_with_tag(
        &mut self,
        exports: Vec<(String, Value)>,
        namespace_tag: &str,
    ) -> Result<Value, JsError> {
        let namespace = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        if let Some(object) = self.object_data_mut(namespace) {
            object.module_namespace = true;
        }
        for (name, value) in exports {
            let atom = self.intern_atom(&name);
            self.set_property(namespace, atom, value)?;
            self.set_property_attributes(
                namespace,
                crate::vm::property_key::PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: true,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        if let Some(symbol) = self.well_known_symbols.get("toStringTag").copied() {
            let module = self
                .heap
                .alloc(Cell::String(namespace_tag.to_string().into()));
            self.set_symbol_property(namespace, symbol, module)?;
            self.set_property_attributes(
                namespace,
                crate::vm::property_key::PropertyKey::symbol(symbol),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        if let Some(object) = self.object_data_mut(namespace) {
            object.set_extensible(false);
        }
        Ok(namespace)
    }

    fn module_static_value(&mut self, constant: Constant) -> Value {
        match constant {
            Constant::Number(value) => Value::number(value),
            Constant::String(value) => self.heap.alloc(Cell::String(value.into())),
            Constant::StringUnits(value) => {
                self.heap.alloc(Cell::String(JsString::from_units(&value)))
            }
            Constant::BigInt(value) => self.heap.alloc(Cell::BigInt(value)),
            Constant::Boolean(true) => Value::TRUE,
            Constant::Boolean(false) => Value::FALSE,
            Constant::Null => Value::NULL,
            Constant::Undefined => Value::UNDEFINED,
        }
    }

    pub(super) fn active_native_env(&self) -> Option<Value> {
        let callee = self.promise.active_native.last().copied()?;
        match self.heap.get(callee) {
            Some(Cell::Function { env, .. }) if !env.is_null() => Some(*env),
            _ => None,
        }
    }

    pub(super) fn promise_resolve_value(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        value: Value,
    ) -> Result<(), JsError> {
        if promise == value {
            let error = self.type_error(p, "Promise cannot resolve to itself".into());
            return self.promise_settle(
                p,
                promise,
                PromiseState::Rejected,
                error.thrown_value().unwrap_or(Value::UNDEFINED),
            );
        }
        if self.object_data(value).is_none() {
            return self.promise_settle(p, promise, PromiseState::Fulfilled, value);
        }
        let then_atom = self.intern_atom("then");
        let then = match self.get_property(p, value, then_atom) {
            Ok(then) => then,
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                return self.promise_settle(p, promise, PromiseState::Rejected, reason);
            }
        };
        if !self.is_function(then) {
            return self.promise_settle(p, promise, PromiseState::Fulfilled, value);
        }
        let job = self.native_with_env(Native::PromiseThenableJob, Value::NULL);
        self.promise.thenable_jobs.insert(
            job,
            ThenableJob {
                then,
                thenable: value,
                promise,
            },
        );
        self.enqueue_job(job, vec![]);
        Ok(())
    }

    pub(super) fn promise_for_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        if self.promise.records.contains_key(&value) {
            let constructor_atom = self.intern_atom("constructor");
            let constructor = self.get_property(p, value, constructor_atom)?;
            if constructor == self.native_value(Native::Promise) {
                return Ok(value);
            }
        }
        let promise = self.promise_object();
        self.promise_resolve_value(p, promise, value)?;
        Ok(promise)
    }

    pub(super) fn promise_then(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        on_fulfilled: Value,
        on_rejected: Value,
    ) -> Result<Value, JsError> {
        let Some(record) = self.promise.records.get(&promise).cloned() else {
            return Err(self.type_error(p, "Promise.prototype method called on non-Promise".into()));
        };
        let constructor = self.promise_species_constructor(p, promise)?;
        let (next, resolve, reject) = self.new_promise_capability(p, constructor)?;
        self.promise
            .reaction_capabilities
            .insert(next, (resolve, reject));
        let reaction = PromiseReaction {
            on_fulfilled: if self.is_function(on_fulfilled) {
                on_fulfilled
            } else {
                Value::UNDEFINED
            },
            on_rejected: if self.is_function(on_rejected) {
                on_rejected
            } else {
                Value::UNDEFINED
            },
            next,
        };
        if record.state == PromiseState::Pending {
            self.promise
                .records
                .get_mut(&promise)
                .unwrap()
                .reactions
                .push(reaction);
        } else {
            self.enqueue_promise_reaction(p, reaction, record.state, record.result);
        }
        Ok(next)
    }

    fn promise_species_constructor(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
    ) -> Result<Value, JsError> {
        let constructor_atom = self.intern_atom("constructor");
        let constructor = self.get_property(p, promise, constructor_atom)?;
        if constructor.is_undefined() {
            return Ok(self.native_value(Native::Promise));
        }
        if !self.is_object_like(constructor) {
            return Err(self.type_error(p, "Promise constructor is not an object".into()));
        }
        let Some(species) = self.well_known_symbols.get("species").copied() else {
            return Ok(self.native_value(Native::Promise));
        };
        let species = self.get_index(p, constructor, species)?;
        if species.is_undefined() || species.is_null() {
            Ok(self.native_value(Native::Promise))
        } else {
            Ok(species)
        }
    }

    pub(super) fn promise_resolve_for_constructor(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        value: Value,
    ) -> Result<Value, JsError> {
        if self.promise.records.contains_key(&value) {
            let constructor_atom = self.intern_atom("constructor");
            if self.get_property(p, value, constructor_atom)? == constructor {
                return Ok(value);
            }
        }
        let (promise, resolve, _) = self.new_promise_capability(p, constructor)?;
        self.call_value(p, resolve, Value::UNDEFINED, &[value])?;
        Ok(promise)
    }

    fn finally_handler_function(
        &mut self,
        handler: Value,
        constructor: Value,
        original_rejected: bool,
    ) -> Value {
        let function = self.native_with_env(Native::PromiseFinallyHandler, Value::UNDEFINED);
        self.promise.finally_handler_callbacks.insert(
            function,
            FinallyHandlerCallback {
                handler,
                constructor,
                original_rejected,
            },
        );
        function
    }

    fn finally_continuation_function(&mut self, original_rejected: bool, original: Value) -> Value {
        let function =
            self.native_with_env(Native::PromiseFinallyContinuationHandler, Value::UNDEFINED);
        self.promise.finally_continuation_callbacks.insert(
            function,
            FinallyContinuationCallback {
                original_rejected,
                original,
            },
        );
        function
    }

    pub(super) fn promise_finally_handler(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let function = *self
            .promise
            .active_native
            .last()
            .ok_or_else(|| JsError("Promise finally handler without callback".into()))?;
        let callback = *self
            .promise
            .finally_handler_callbacks
            .get(&function)
            .ok_or_else(|| JsError("stale Promise finally handler".into()))?;
        let original = args.first().copied().unwrap_or(Value::UNDEFINED);
        let cleanup = self.call_value(p, callback.handler, Value::UNDEFINED, &[])?;
        let promise = self.promise_resolve_for_constructor(p, callback.constructor, cleanup)?;
        let then_atom = self.intern_atom("then");
        let then = self.get_property(p, promise, then_atom)?;
        if !self.is_function(then) {
            return Err(self.type_error(p, "Promise cleanup then is not callable".into()));
        }
        let continuation = self.finally_continuation_function(callback.original_rejected, original);
        self.call_value(p, then, promise, &[continuation])
    }

    pub(super) fn promise_finally_continuation_handler(
        &mut self,
        _p: &ResidualProgram,
        _args: &[Value],
    ) -> Result<Value, JsError> {
        let function = *self
            .promise
            .active_native
            .last()
            .ok_or_else(|| JsError("Promise finally continuation without callback".into()))?;
        let callback = *self
            .promise
            .finally_continuation_callbacks
            .get(&function)
            .ok_or_else(|| JsError("stale Promise finally continuation".into()))?;
        if callback.original_rejected {
            return Err(JsError::thrown(
                callback.original,
                "Promise was rejected before finally".into(),
            ));
        }
        Ok(callback.original)
    }

    fn promise_catch(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if receiver.is_null() || receiver.is_undefined() {
            return Err(
                self.type_error(p, "Promise.prototype.catch called on nullish value".into())
            );
        }
        let then_atom = self.intern_atom("then");
        let then = self.get_property(p, receiver, then_atom)?;
        if !self.is_function(then) {
            return Err(self.type_error(p, "Promise.prototype.catch then is not callable".into()));
        }
        self.call_value(
            p,
            then,
            receiver,
            &[
                Value::UNDEFINED,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            ],
        )
    }

    fn promise_finally(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if receiver.is_null() || receiver.is_undefined() {
            return Err(self.type_error(
                p,
                "Promise.prototype.finally called on nullish value".into(),
            ));
        }
        let then_atom = self.intern_atom("then");
        let then = self.get_property(p, receiver, then_atom)?;
        if !self.is_function(then) {
            return Err(self.type_error(p, "Promise.prototype.finally then is not callable".into()));
        }
        let handler = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(handler) {
            return self.call_value(p, then, receiver, &[handler, handler]);
        }
        let constructor = self.promise_species_constructor(p, receiver)?;
        let fulfilled = self.finally_handler_function(handler, constructor, false);
        let rejected = self.finally_handler_function(handler, constructor, true);
        self.call_value(p, then, receiver, &[fulfilled, rejected])
    }
}
