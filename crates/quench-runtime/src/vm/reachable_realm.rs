//! The closed world over the build-time realm (C5 C2): what a compiled program can reach of the
//! realm the runtime installs, computed once in the compile process.
//!
//! The realm is initialized with `BuiltinTable::ALL`, so every intrinsic exists as an object. The
//! roots are what the program names: its global bindings and the natives its image folds
//! (`RealmRoots`), the intrinsic prototypes of every value kind its opcodes create, and the
//! prototypes and globals the runtime itself reaches as it runs (errors, iteration). The closure
//! follows every own property (data and accessor) and the prototype of every reachable object.
//! A reachable dynamic-source entry (`eval`, `Function`) can name any global at run time, so it
//! reaches the global object, which makes every global a root.
//!
//! Nothing here reads the program's spelling: a family is reached when the closure reaches one of
//! its realm objects, and a native is reached when the closure holds its function.

use super::*;
use super::builtins::RealmScope;
use crate::bytecode::{activation_reads_this, Constant, Op, ProgramKind};
use rustc_hash::FxHashSet;
use std::collections::BTreeSet;
use std::rc::Rc;

/// The realm a program reaches: the natives its native table keeps, the optional families it
/// installs, and the realm's capabilities the image must link.
#[derive(Clone, Debug)]
pub struct ReachableRealm {
    /// The natives whose entries the image's table keeps; every other native is `None` in it.
    pub natives: BTreeSet<Native>,
    /// The optional intrinsic families the closure reaches (`BuiltinTable::families`).
    pub families: BuiltinFamilies,
    /// Whether the image can make code from a string at run time (`eval`, `Function`).
    pub makes_code: bool,
    /// Whether `Function.prototype.toString` is reachable, so source text is kept.
    pub needs_source_text: bool,
    /// Whether an Error stack position is read or captured.
    pub needs_stack_positions: bool,
    /// Whether the pattern compiler is reachable (the `RegExp` constructor and `compile`).
    pub needs_regexp_compiler: bool,
    /// Whether Intl or Temporal is reachable (their globals or a locale-sensitive native).
    pub needs_intl: bool,
    /// Whether a local-time Date method is reachable.
    pub needs_tz: bool,
    /// The realm objects the closure reaches.
    pub objects: usize,
}

/// The roots a closed world starts from: the global bindings the program reads by name and the
/// natives its image folds (`Demand::globals`, `Demand::natives`).
pub struct RealmRoots<'a> {
    pub globals: &'a BTreeSet<Box<str>>,
    pub natives: &'a BTreeSet<Native>,
}

/// The realm every closed world is computed over: every intrinsic family installed.
static COMPLETE_REALM: BuiltinTable = BuiltinTable::ALL;

/// Computes what `programs` (the entry first, then its units) reach of the build-time realm.
pub fn reachable_realm(programs: &[&ResidualProgram], roots: RealmRoots<'_>) -> Result<ReachableRealm, JsError> {
    let (entry, _) = programs.split_first().ok_or_else(|| JsError("the closed world needs an entry program".into()))?;
    let mut vm = Vm::new_static_with_builtins(SystemHost, &COMPLETE_REALM);
    vm.initialize_shared(&Rc::new((*entry).clone()), RealmScope::Complete)?;
    vm.close_world(entry, programs, roots)
}

/// The closure's state: every realm value it has reached, the values still to visit, and the
/// natives among the reached functions.
#[derive(Default)]
struct Closure {
    seen: FxHashSet<Value>,
    pending: Vec<Value>,
    natives: BTreeSet<Native>,
}

/// The global names the runtime reaches as it runs whatever the program names: the error
/// constructors, since any operation can throw.
const RUNTIME_GLOBALS: &[&str] = &[
    "Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError", "EvalError", "URIError", "AggregateError",
];

/// The global names whose own values prove a family reached.
const FAMILY_NAMES: &[(BuiltinFamilies, &[&str])] = &[
    (BuiltinFamilies::REGEXP, &["RegExp"]),
    (BuiltinFamilies::TYPED_MEMORY, &[
        "ArrayBuffer", "SharedArrayBuffer", "DataView", "Atomics", "Int8Array", "Uint8Array", "Uint8ClampedArray",
        "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array",
    ]),
    (BuiltinFamilies::COLLECTIONS, &["Map", "Set", "WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry"]),
    (BuiltinFamilies::DATE, &["Date"]),
    (BuiltinFamilies::BIGINT, &["BigInt"]),
    (BuiltinFamilies::JSON, &["JSON"]),
    (BuiltinFamilies::PROMISE_ASYNC, &["Promise", "queueMicrotask"]),
    (BuiltinFamilies::SHADOW_REALM, &["ShadowRealm"]),
];

/// The natives that make code from a string at run time.
fn is_dynamic_source(native: &Native) -> bool {
    matches!(native, Native::Eval | Native::Function | Native::EvalScript)
}

impl Vm {
    fn close_world(&mut self, entry: &ResidualProgram, programs: &[&ResidualProgram], roots: RealmRoots<'_>) -> Result<ReachableRealm, JsError> {
        let mut closure = Closure::default();
        self.root_runtime(entry, &mut closure)?;
        let eval = programs.iter().any(|program| program.kind == ProgramKind::Eval);
        let mut global_root = eval;
        for program in programs {
            self.root_constants(entry, &mut closure, program)?;
            self.root_functions(&mut closure, program, &mut global_root);
        }
        for name in roots.globals {
            self.root_global(entry, &mut closure, name)?;
        }
        for native in roots.natives {
            let value = self.native_value(*native);
            self.enqueue(&mut closure, value);
        }
        self.close_reachable(entry, &mut closure, global_root)?;
        let makes_code = eval || closure.natives.iter().any(is_dynamic_source);
        Ok(self.realm_summary(&closure, makes_code))
    }

    /// Roots what the runtime's own code reaches whatever the program names: the error and `with`
    /// and iterator-close bindings, and the prototypes every runtime-made value reaches.
    fn root_runtime(&mut self, entry: &ResidualProgram, closure: &mut Closure) -> Result<(), JsError> {
        for name in RUNTIME_GLOBALS.iter().copied().chain([
            crate::bytecode::ITERATOR_CLOSE_BINDING, crate::bytecode::WITH_ENTER_BINDING, crate::bytecode::WITH_EXIT_BINDING,
        ]) {
            self.root_global(entry, closure, name)?;
        }
        for value in [self.object_proto, self.function_proto, self.array_proto, self.string_proto, self.iterator_proto, self.array_iterator_proto, self.string_iterator_proto] {
            self.enqueue(closure, value);
        }
        Ok(())
    }

    /// Visits the closure to its fixpoint. Code made from a string names any global, so a reached
    /// `eval` or `Function` (or a program that runs `eval`) makes the global object a root.
    fn close_reachable(&mut self, entry: &ResidualProgram, closure: &mut Closure, global_root: bool) -> Result<(), JsError> {
        let globals = self.realm.globals;
        if global_root {
            self.enqueue(closure, globals);
        }
        loop {
            self.drain(entry, closure)?;
            if closure.seen.contains(&globals) || !closure.natives.iter().any(is_dynamic_source) {
                return Ok(());
            }
            self.enqueue(closure, globals);
        }
    }

    /// Roots the prototypes of the kinds the program's constants and functions create.
    fn root_constants(&mut self, entry: &ResidualProgram, closure: &mut Closure, program: &ResidualProgram) -> Result<(), JsError> {
        for constant in (&program.constants).into_iter() {
            let name = match constant {
                Constant::String(_) | Constant::StringUnits(_) => "String",
                Constant::Number(_) => "Number",
                Constant::Boolean(_) => "Boolean",
                Constant::BigInt(_) => "BigInt",
                _ => continue,
            };
            self.root_global(entry, closure, name)?;
        }
        Ok(())
    }

    /// Roots the prototypes of the kinds the program's functions and opcodes create.
    fn root_functions(&mut self, closure: &mut Closure, program: &ResidualProgram, global_root: &mut bool) {
        for index in 0..program.function_count() {
            let Some(function) = program.function_get(index) else { continue };
            if function.is_async {
                self.enqueue(closure, self.promise_proto);
            }
            if function.is_generator {
                self.enqueue(closure, self.generator_proto);
            }
            if function.is_async && function.is_generator {
                self.enqueue(closure, self.async_generator_proto);
            }
            // A sloppy function that reads `this` reads the global object when called without a receiver.
            if !function.strict && activation_reads_this(program, function) {
                *global_root = true;
            }
            if function.has_dynamic_scope_access() {
                *global_root = true;
            }
            for instruction in function.instructions() {
                match instruction.op() {
                    Op::MakeArray | Op::MakeConstArray | Op::MakeLiteralTemplate => self.enqueue(closure, self.array_proto),
                    Op::MakeObject | Op::MakeObjectLiteral => self.enqueue(closure, self.object_proto),
                    Op::MakeClosure => self.enqueue(closure, self.function_proto),
                    Op::CreateRegExpLiteral => self.enqueue(closure, self.regexp_proto),
                    Op::Yield | Op::YieldStar => self.enqueue(closure, self.generator_proto),
                    Op::Await => self.enqueue(closure, self.promise_proto),
                    Op::IteratorClose | Op::IteratorCleanupPush | Op::IteratorCleanupPop => {
                        self.enqueue(closure, self.iterator_proto);
                        self.enqueue(closure, self.array_iterator_proto);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Roots the global binding `name`, materializing it first when it is a lazy data property.
    fn root_global(&mut self, entry: &ResidualProgram, closure: &mut Closure, name: &str) -> Result<(), JsError> {
        let Some(atom) = self.lookup_atom(name) else { return Ok(()) };
        let key = PropertyKey::string(atom);
        let globals = self.realm.globals;
        if self.is_pending_lazy_data_property(globals, key) {
            self.materialize_lazy_data_property(entry, globals, key)?;
        }
        for value in self.own_values(globals, key) {
            self.enqueue(closure, value);
        }
        Ok(())
    }

    fn enqueue(&self, closure: &mut Closure, value: Value) {
        if self.is_object_like(value) && closure.seen.insert(value) {
            closure.pending.push(value);
        }
    }

    /// Visits every reached value until the closure holds no unvisited one.
    fn drain(&mut self, entry: &ResidualProgram, closure: &mut Closure) -> Result<(), JsError> {
        while let Some(holder) = closure.pending.pop() {
            if let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = self.heap.function(holder) {
                closure.natives.insert(*native);
            }
            let Some(shape) = self.object_data(holder).map(|data| data.shape()) else { continue };
            // Materialize the lazy members before the keys are read, so an initializer's members are reached.
            for key in self.shape_keys(shape) {
                if self.is_pending_lazy_data_property(holder, key) {
                    self.materialize_lazy_data_property(entry, holder, key)?;
                }
            }
            let shape = self.object_data(holder).map_or(shape, |data| data.shape());
            for key in self.shape_keys(shape) {
                for value in self.own_values(holder, key) {
                    self.enqueue(closure, value);
                }
            }
            if let Some(proto) = self.object_data(holder).map(|data| data.proto()) {
                self.enqueue(closure, proto);
            }
        }
        Ok(())
    }

    /// The values of `holder`'s own property `key`: its getter and setter, or its data value.
    fn own_values(&self, holder: Value, key: PropertyKey) -> Vec<Value> {
        let Some(data) = self.object_data(holder) else { return Vec::new() };
        let Some((_, slot)) = self.object_property_slot(holder, key) else { return Vec::new() };
        let Some(attributes) = self.slot_attributes(data, slot) else { return Vec::new() };
        let mut values: Vec<Value> = attributes.getter.into_iter().chain(attributes.setter).collect();
        if !attributes.accessor {
            values.extend(self.slot_data_value(data, slot));
        }
        values
    }

    /// Whether the closure reaches any of `names`' global values or any of `protos`.
    fn reaches(&self, closure: &Closure, names: &[&str], protos: &[Value]) -> bool {
        let globals = self.realm.globals;
        protos.iter().any(|value| closure.seen.contains(value))
            || names.iter().any(|name| {
                self.lookup_atom(name).is_some_and(|atom| {
                    self.own_values(globals, PropertyKey::string(atom)).iter().any(|value| closure.seen.contains(value))
                })
            })
    }

    fn realm_summary(&self, closure: &Closure, makes_code: bool) -> ReachableRealm {
        let natives = &closure.natives;
        let names: Vec<String> = natives.iter().map(|native| format!("{native:?}")).collect();
        // `Object.prototype.toLocaleString` only calls `toString`, so it proves no Intl layer.
        let intl_name = |name: &String| name.starts_with("Intl") || name.starts_with("Temporal") || (name.contains("Locale") && !name.starts_with("Object"));
        let local_time = |name: &String| {
            const CLOCK_ONLY: &[&str] = &[
                "DateNow", "DateGetTime", "DateValueOf", "DateSetTime", "DateToISOString", "DateToJSON", "DateToTemporalInstant",
            ];
            name.starts_with("Date") && !name.contains("UTC") && !CLOCK_ONLY.contains(&name.as_str())
        };
        let mut families = BuiltinFamilies::NONE;
        for (family, names) in FAMILY_NAMES {
            if self.reaches(closure, names, &self.family_protos(*family)) {
                families = families.union(*family);
            }
        }
        // A string pattern makes a RegExp in match, matchAll and search.
        if natives.iter().any(|native| matches!(native, Native::StringMatch | Native::StringMatchAll | Native::StringSearch)) {
            families = families.union(BuiltinFamilies::REGEXP);
        }
        ReachableRealm {
            natives: natives.clone(),
            families,
            makes_code,
            needs_source_text: natives.contains(&Native::FunctionToString),
            needs_stack_positions: natives.iter().any(|native| matches!(native, Native::ErrorStackGetter | Native::ErrorStackSetter | Native::ErrorCaptureStackTrace)),
            needs_regexp_compiler: natives.iter().any(|native| matches!(native, Native::RegExp | Native::RegExpCompile)),
            needs_intl: names.iter().any(intl_name) || self.reaches(closure, &["Intl", "Temporal"], &[]),
            needs_tz: names.iter().any(local_time),
            objects: closure.seen.len(),
        }
    }

    /// The realm objects a family's installation creates, besides its global names.
    fn family_protos(&self, family: BuiltinFamilies) -> Vec<Value> {
        if family == BuiltinFamilies::REGEXP {
            vec![self.regexp_proto, self.regexp_string_iterator_proto]
        } else if family == BuiltinFamilies::TYPED_MEMORY {
            vec![self.typed_array_proto, self.array_buffer_proto, self.shared_array_buffer_proto, self.data_view_proto]
        } else if family == BuiltinFamilies::COLLECTIONS {
            vec![self.map_proto, self.set_proto, self.weak_map_proto, self.weak_set_proto, self.weak_ref_proto, self.finalization_registry_proto, self.map_iterator_proto, self.set_iterator_proto]
        } else if family == BuiltinFamilies::PROMISE_ASYNC {
            vec![self.promise_proto, self.async_generator_proto, self.async_iterator_proto, self.async_from_sync_iterator_proto]
        } else if family == BuiltinFamilies::SHADOW_REALM {
            vec![self.shadow_realm_proto]
        } else {
            Vec::new()
        }
    }
}
