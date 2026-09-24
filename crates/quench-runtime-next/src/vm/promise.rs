use super::activation::ContinuationId;
use super::module::{ModuleOutcome, ModulePhase, ModuleRecord};
use super::*;
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
    Constant(Constant),
    Cached(Value),
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
            StaticModuleValue::Namespace { name: left, .. },
            StaticModuleValue::Namespace { name: right, .. },
        ) => {
            crate::module_identity::normalize(std::path::Path::new(left))
                == crate::module_identity::normalize(std::path::Path::new(right))
        }
        (StaticModuleValue::Cached(left), StaticModuleValue::Cached(right)) => left == right,
        _ => false,
    }
}

fn native_length(kind: Native) -> Option<f64> {
    if let Some(length) = super::date::date_native_length(kind) {
        return Some(length);
    }
    Some(match kind {
        Native::Object => 1.0,
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
        Native::ObjectHasOwn => 2.0,
        Native::ObjectGetOwnPropertyDescriptor | Native::ObjectIs => 2.0,
        Native::ObjectCreate => 2.0,
        Native::ObjectDefineProperties | Native::ObjectAssign => 2.0,
        Native::ObjectFromEntries => 1.0,
        Native::ObjectDefineProperty => 3.0,
        Native::ObjectSetPrototypeOf => 2.0,
        Native::ReflectHas | Native::ReflectApply => 2.0,
        Native::ObjectPrototypeHasOwnProperty
        | Native::ObjectPrototypePropertyIsEnumerable
        | Native::ObjectPrototypeIsPrototypeOf => 1.0,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AggregateMode {
    All,
    Race,
    AllSettled,
    Any,
}

#[derive(Clone, Debug)]
pub(super) struct AggregateRecord {
    pub(super) mode: AggregateMode,
    pub(super) output: Value,
    pub(super) remaining: usize,
    pub(super) values: Vec<Value>,
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

pub(super) struct PromiseRuntime {
    pub(super) proto: Value,
    pub(super) records: FxHashMap<Value, PromiseRecord>,
    pub(super) jobs: FxHashMap<Value, PromiseJob>,
    pub(super) thenable_jobs: FxHashMap<Value, ThenableJob>,
    pub(super) finally_jobs: FxHashMap<Value, FinallyJob>,
    pub(super) finally_continuation_jobs: FxHashMap<Value, FinallyContinuationJob>,
    pub(super) aggregates: FxHashMap<Value, AggregateRecord>,
    pub(super) aggregate_jobs: FxHashMap<Value, AggregateJob>,
    pub(super) async_resume_jobs: FxHashMap<Value, AsyncResumeJob>,
    pub(super) modules: FxHashMap<String, ModuleRecord>,
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
            aggregates: FxHashMap::default(),
            aggregate_jobs: FxHashMap::default(),
            async_resume_jobs: FxHashMap::default(),
            modules: FxHashMap::default(),
            waiting_static_modules: Vec::new(),
            active_native: vec![],
        }
    }
}

impl<H: Host> Vm<H> {
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
        let promise_resolver =
            !env.is_null() && matches!(kind, Native::PromiseResolve | Native::PromiseReject);
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
        self.set_named(program, promise, "prototype", self.promise.proto)?;
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
        self.set_named(
            program,
            self.promise.proto,
            "then",
            self.native_value(Native::PromiseThen),
        )?;
        self.set_named(
            program,
            self.promise.proto,
            "catch",
            self.native_value(Native::PromiseCatch),
        )?;
        self.set_named(
            program,
            self.promise.proto,
            "finally",
            self.native_value(Native::PromiseFinally),
        )?;
        self.set_named(
            program,
            promise,
            "resolve",
            self.native_value(Native::PromiseResolve),
        )?;
        self.set_named(
            program,
            promise,
            "reject",
            self.native_value(Native::PromiseReject),
        )?;
        self.set_named(
            program,
            promise,
            "all",
            self.native_value(Native::PromiseAll),
        )?;
        self.set_named(
            program,
            promise,
            "race",
            self.native_value(Native::PromiseRace),
        )?;
        self.set_named(
            program,
            promise,
            "allSettled",
            self.native_value(Native::PromiseAllSettled),
        )?;
        self.set_named(
            program,
            promise,
            "any",
            self.native_value(Native::PromiseAny),
        )?;
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
            return Err(JsError("Promise resolver is not a function".into()));
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

    pub(super) fn call_promise_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::DynamicImport => self.dynamic_import(p, args),
            Native::Promise => Err(JsError(
                "Promise constructor must be called with new".into(),
            )),
            Native::PromiseResolve => {
                if let Some(promise) = self.active_native_env() {
                    self.promise_resolve_value(
                        p,
                        promise,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                    )?;
                    Ok(Value::UNDEFINED)
                } else {
                    if let Some(value) = args.first().copied()
                        && self.promise.records.contains_key(&value)
                    {
                        return Ok(value);
                    }
                    let promise = self.promise_object();
                    self.promise_resolve_value(
                        p,
                        promise,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
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
                    let promise = self.promise_object();
                    self.promise_settle(
                        p,
                        promise,
                        PromiseState::Rejected,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
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
            Native::PromiseCatch => self.promise_then(
                p,
                this,
                Value::UNDEFINED,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::PromiseFinally => {
                self.promise_finally(p, this, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::PromiseAll => self.promise_aggregate(p, args, AggregateMode::All),
            Native::PromiseRace => self.promise_aggregate(p, args, AggregateMode::Race),
            Native::PromiseAllSettled => self.promise_aggregate(p, args, AggregateMode::AllSettled),
            Native::PromiseAny => self.promise_aggregate(p, args, AggregateMode::Any),
            Native::PromiseReactionJob => {
                self.promise_reaction_job(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::PromiseThenableJob => self.promise_thenable_job(p),
            Native::PromiseFinallyJob => self.promise_finally_job(p),
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
                        let mut active = FxHashSet::default();
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
                        value: Value::UNDEFINED,
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
        let Some(record) = self.promise.modules.get_mut(&key) else {
            return Ok(());
        };
        match result {
            Ok(_) => {
                if let Some((namespace, waiters)) = record.evaluate_root() {
                    for waiter in waiters {
                        self.promise_resolve_value(p, waiter, namespace)?;
                    }
                }
            }
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.to_string())));
                if let Some(waiters) = record.fail(reason) {
                    for waiter in waiters {
                        self.promise_settle(p, waiter, PromiseState::Rejected, reason)?;
                    }
                }
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
            return self.syntax_error_result(
                p,
                "source phase import is not available for source text modules",
            );
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

    fn evaluate_javascript_module(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<Value, JsError> {
        if crate::Engine::static_module_has_early_error(&module.source) {
            return self.syntax_error_result(p, "module source has an early error");
        }
        if let Some(plan) = crate::Engine::static_module_plan(&module.source, &module.name) {
            let mut active = FxHashSet::default();
            active.insert(crate::module_identity::normalize(std::path::Path::new(
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
            .locals
            .into_iter()
            .map(|(_, exported)| exported)
            .collect::<Vec<_>>();
        if let Some(exports) = crate::Engine::static_module_exports(&module.source, &module.name) {
            names.extend(exports.into_iter().map(|(name, _)| name));
        }
        names.sort_by_key(|name| name.encode_utf16().collect::<Vec<_>>());
        names.dedup();
        let namespace = self.module_namespace(
            names
                .into_iter()
                .map(|name| (name, Value::UNDEFINED))
                .collect(),
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
        if let Some(object) = self.object_data_mut(namespace) {
            object.deferred_module = None;
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
        if crate::Engine::static_module_plan(&module.source, &module.name)
            .is_some_and(|plan| plan.reexports.is_empty())
            && let Some(exports) = crate::Engine::module_export_names(&module.source, &module.name)
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
        if p.module_requests.is_empty() {
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
        let mut active = FxHashSet::default();
        active.insert(crate::module_identity::normalize(std::path::Path::new(
            &p.source_name,
        )));
        let outer_batch = std::mem::replace(&mut self.deferred_dependency_batch, true);
        let requests =
            self.evaluate_module_requests(p, &p.source_name, &p.module_requests, &mut active);
        self.deferred_dependency_batch = outer_batch;
        requests?;
        let mut linking = ActiveModuleExports::default();
        let imports = self.resolve_module_import_values(
            p,
            p,
            &p.source_name,
            &p.module_imports,
            &mut linking,
        )?;
        self.programs
            .set_module_import_values(ProgramId::MAIN, imports);
        if !outer_batch {
            self.drain_jobs(p)?;
            self.advance_static_module_jobs(p)?;
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
    ) -> Result<Vec<(u16, Value)>, JsError> {
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
                    if self
                        .object_data(namespace)
                        .is_some_and(Object::is_module_namespace)
                        && self.own_property(namespace, atom).is_none()
                    {
                        return self
                            .syntax_error_result(p, "module import binding could not be resolved")
                            .map(|_| Vec::new());
                    }
                    self.get_property(p, namespace, atom)?
                }
            };
            let local = self
                .lookup_atom(&import.local)
                .and_then(|atom| {
                    root.local_atoms
                        .iter()
                        .position(|candidate| *candidate == atom)
                })
                .and_then(|slot| u16::try_from(slot).ok())
                .ok_or_else(|| self.type_error(p, "module import slot is unavailable".into()))?;
            values.push((local, value));
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
        active: &mut FxHashSet<std::path::PathBuf>,
    ) -> Result<(), JsError> {
        for request in requests {
            match request.phase {
                crate::bytecode::ModuleRequestPhase::Evaluation => {
                    self.evaluate_static_module_request(p, referrer, request, active)?;
                }
                crate::bytecode::ModuleRequestPhase::Defer => {
                    self.prepare_deferred_module_request(p, referrer, request)?;
                }
                crate::bytecode::ModuleRequestPhase::Source => {
                    return Err(self.type_error(p, "source-phase imports are unsupported".into()));
                }
            }
        }
        Ok(())
    }

    fn prepare_deferred_module_request(
        &mut self,
        p: &ResidualProgram,
        referrer: &str,
        request: &crate::bytecode::ModuleRequest,
    ) -> Result<(), JsError> {
        let module = self
            .host
            .resolve_dynamic_import(referrer, &request.source)
            .map_err(|message| self.type_error(p, message))?
            .ok_or_else(|| self.type_error(p, "deferred module request was not resolved".into()))?;
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
            let mut active = FxHashSet::default();
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
        referrer: &str,
        request: &crate::bytecode::ModuleRequest,
        active: &mut FxHashSet<std::path::PathBuf>,
    ) -> Result<(), JsError> {
        let module = match self.host.resolve_dynamic_import(referrer, &request.source) {
            Ok(Some(module)) => module,
            Ok(None) => {
                return Err(self.type_error(p, "static module request was not resolved".into()));
            }
            Err(message) => return Err(self.type_error(p, message)),
        };
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
        active: &mut FxHashSet<std::path::PathBuf>,
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
        active.insert(identity);
        let result = self.evaluate_static_module_body(p, &module, &plan, active);
        active.remove(&crate::module_identity::normalize(std::path::Path::new(
            &module.name,
        )));
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
        active: &mut FxHashSet<std::path::PathBuf>,
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
        for waiter in waiters {
            self.promise_settle(p, waiter, state, value)?;
        }
        if state == PromiseState::Rejected {
            return Err(JsError::thrown(
                value,
                "static module evaluation failed".into(),
            ));
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
            let mut active = FxHashSet::default();
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
            .modules
            .iter()
            .filter_map(|(key, module)| {
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
            let phase = self
                .promise
                .modules
                .get(&module_cache_key(&dependency.name, "javascript"))
                .map(ModuleRecord::phase);
            if matches!(
                phase,
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
        self.programs.set_module_import_values(program_id, imports);
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
                values.push((
                    exported,
                    StaticModuleValue::Binding {
                        program: program_id,
                        slot,
                        value,
                    },
                ));
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

    fn resolve_static_module_graph(
        &mut self,
        p: &ResidualProgram,
        module: ModuleSource,
        active: &mut ActiveModuleExports,
    ) -> Result<StaticModuleGraph, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if let Some(exports) = active.get(&identity) {
            return Ok(StaticModuleGraph::Linked {
                name: module.name,
                exports: exports.clone(),
                incomplete: true,
            });
        }
        if let Some(exports) = crate::Engine::static_module_exports(&module.source, &module.name) {
            return Ok(StaticModuleGraph::Linked {
                name: module.name,
                exports: exports
                    .into_iter()
                    .map(|(name, value)| (name, StaticModuleValue::Constant(value)))
                    .collect(),
                incomplete: false,
            });
        }
        if let Some(plan) = crate::Engine::static_module_plan(&module.source, &module.name)
            && !plan.reexports.is_empty()
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
        let locals = if plan.locals.is_empty() {
            Vec::new()
        } else {
            self.evaluate_module_locals(p, module, plan.locals, active)?
        };
        self.resolve_static_module_links(p, module, locals, plan.reexports, active)
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
                StaticModuleValue::Constant(value) => self.module_static_value(value),
                StaticModuleValue::Cached(value) => value,
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
        if let Some(tag) = self.well_known_symbols.get("toStringTag").copied() {
            let module = self.heap.alloc(Cell::String("Module".into()));
            self.set_symbol_property(namespace, tag, module)?;
            self.set_property_attributes(
                namespace,
                crate::vm::property_key::PropertyKey::symbol(tag),
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
            let error = self
                .heap
                .alloc(Cell::Error("Promise cannot resolve to itself".into()));
            return self.promise_settle(p, promise, PromiseState::Rejected, error);
        }
        if let Some(record) = self.promise.records.get(&value).cloned() {
            let reaction = PromiseReaction {
                on_fulfilled: Value::UNDEFINED,
                on_rejected: Value::UNDEFINED,
                next: promise,
            };
            if record.state == PromiseState::Pending {
                self.promise
                    .records
                    .get_mut(&value)
                    .unwrap()
                    .reactions
                    .push(reaction);
            } else {
                self.enqueue_promise_reaction(p, reaction, record.state, record.result);
            }
            return Ok(());
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
            return Ok(value);
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
            return Err(JsError(
                "Promise.prototype method called on non-Promise".into(),
            ));
        };
        let next = self.promise_object();
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

    fn promise_finally(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        handler: Value,
    ) -> Result<Value, JsError> {
        if !self.is_function(handler) {
            return self.promise_then(p, promise, Value::UNDEFINED, Value::UNDEFINED);
        }
        let Some(record) = self.promise.records.get(&promise).cloned() else {
            return Err(JsError(
                "Promise.prototype method called on non-Promise".into(),
            ));
        };
        let next = self.promise_object();
        let reaction = FinallyReaction { handler, next };
        if record.state == PromiseState::Pending {
            self.promise
                .records
                .get_mut(&promise)
                .unwrap()
                .finally_reactions
                .push(reaction);
        } else {
            self.enqueue_promise_finally(reaction, record.state, record.result);
        }
        Ok(next)
    }
}
