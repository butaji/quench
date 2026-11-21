use super::property_key::PropertyKey;
use super::wtf16::JsString;
use super::*;

/// Marks a cached atom field that has not been interned since the last program reset.
pub(super) const NO_CACHED_ATOM: Atom = Atom::MAX;
/// `Math`'s functions, in installation order: derived from the rows of `native_rows`.
const MATH_FUNCTIONS: &[(&str, Native)] = super::native_rows::MATH_FUNCTIONS;
/// The number of natives that are shared function objects: `Native::ALL` less the unshared ones.
const fn shared_native_count() -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < Native::ALL.len() {
        if Native::ALL[index].has_shared_function() {
            count += 1;
        }
        index += 1;
    }
    count
}

/// The natives that are shared function objects, in discriminant order. Each row's native is
/// among them: `native_value` asserts that a native has a row or is listed here.
const NATIVES: [Native; shared_native_count()] = {
    let mut natives = [Native::Print; shared_native_count()];
    let mut count = 0;
    let mut index = 0;
    while index < Native::ALL.len() {
        if Native::ALL[index].has_shared_function() {
            natives[count] = Native::ALL[index];
            count += 1;
        }
        index += 1;
    }
    natives
};
/// The function objects every realm shares, one per `NATIVES` entry. Each is built on its
/// first read (`Vm::native_value`): a realm's installation reads fewer than half of them,
/// and a program reads the rest only through the builtins it uses.
pub(super) struct NativeTable {
    /// The realm that owns the table. Its function prototype is every entry's prototype,
    /// whichever realm is active when the entry is first read.
    realm: Value,
    /// Each entry's function, indexed by `Native` discriminant; `NULL` until first read.
    functions: Vec<Value>,
}

crate::snapshot::snap_struct!(NativeTable { realm, functions });

impl Default for NativeTable {
    fn default() -> Self {
        Self { realm: Value::NULL, functions: Vec::new() }
    }
}

impl NativeTable {
    /// Empties the table for a realm whose entries are not yet built.
    fn reset(&mut self, realm: Value) {
        self.realm = realm;
        self.functions.clear();
    }

    #[inline(always)]
    pub(super) fn get(&self, kind: Native) -> Option<Value> {
        self.functions
            .get(kind as usize)
            .copied()
            .filter(|function| !function.is_null())
    }

    fn insert(&mut self, kind: Native, function: Value) {
        let index = kind as usize;
        if self.functions.len() <= index {
            self.functions.resize(index + 1, Value::NULL);
        }
        self.functions[index] = function;
    }

    /// The values the table keeps alive: its realm and the entries built so far.
    pub(super) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        std::iter::once(self.realm)
            .chain(self.functions.iter().copied())
            .filter(|value| !value.is_null())
    }
}

/// How much of a realm a VM installs when it initializes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RealmScope {
    /// Every builtin: the VM will run JavaScript.
    Complete,
    /// The prototypes, native functions and error intrinsics Wasm instances,
    /// their exports and their traps use. A VM first initialized for Wasm
    /// installs only these; executing JavaScript initializes a complete
    /// realm afresh (`Vm::execute`).
    Core,
}

#[cfg(test)]
mod namespace_tests {
    use super::*;
    use crate::{Engine, test_host::TestHost};

    #[test]
    fn a_new_realm_can_use_the_creators_unobserved_math_namespace() {
        for compile in [Engine::specialize as fn(&str, &str) -> _, Engine::specialize_unspecialized] {
            let program = compile("var other = $262.createRealm().global; print(typeof other.Math); print(typeof other.Math.sin); print(other.Math.abs(-4));", "lazy-realm-math.js").unwrap();
            for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                vm.set_execution_mode(mode);
                vm.execute(&program).unwrap();
                assert_eq!(&*vm.host_as::<TestHost>().lines(), &["object", "function", "4"]);
            }
        }
    }

    #[test]
    fn independent_builtin_namespaces_remain_unallocated_until_observed() {
        let program = Engine::specialize("0", "namespace-preparation.js").unwrap();
        let mut vm = Vm::new(TestHost::default());
        vm.initialize(&program).unwrap();
        for name in ["Math", "Temporal", "Atomics", "Intl", "WebAssembly", "Reflect", "Proxy", "WeakRef", "FinalizationRegistry", "DataView"] {
            let atom = vm.lookup_atom(name).unwrap();
            assert!(vm.is_pending_lazy_data_property(vm.realm.globals, PropertyKey::string(atom)), "{name}");
        }
        assert!(vm.natives.get(Native::MathSin).is_none());
    }
}

impl Vm {
    pub(super) fn install_builtins(
        &mut self,
        program: &ResidualProgram,
        scope: RealmScope,
    ) -> Result<(), JsError> {
        self.install_prototypes();
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_prototypes");
        self.natives.reset(self.realm.globals);
        // A shared native proved before is no longer in the table (`RealmNativeProofs`).
        self.realm_native_proofs.forget();
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_native_function_table");
        // Error intrinsics must exist before any guarded initialization step.
        self.install_error_intrinsics(program)?;
        self.install_throw_type_error_for_realm(self.realm.globals)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_core_errors");
        if scope == RealmScope::Core {
            return Ok(());
        }
        self.install_object(program)?;
        self.install_console(program)?;
        self.install_array(program)?;
        self.install_array_buffer(program)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_object_arrays");
        self.install_typed_array(program)?;
        self.install_lazy_builtin_namespace(program, "DataView", Native::DataViewInitialize)?;
        self.install_lazy_builtin_namespace(program, "Atomics", Native::AtomicsInitialize)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_typed_memory");
        self.install_collections(program)?;
        self.install_weak_collections(program)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_collections");
        self.install_iterators(program)?;
        let native_iterator_close = self.native_value(Native::IteratorClose);
        self.global(
            program,
            crate::bytecode::ITERATOR_CLOSE_BINDING,
            native_iterator_close,
        )?;
        for (name, value) in immutable_globals() {
            self.global(program, name, value)?;
            let atom = self.intern_atom(name);
            self.set_property_attributes(self.realm.globals, property_key::PropertyKey::string(atom), PropertyAttributes {
                writable: false, enumerable: false, configurable: false, accessor: false, getter: None, setter: None,
            });
        }
        let native_print = self.native_value(Native::Print);
        self.global(program, "print", native_print)?;
        self.set_builtin_named(program, self.realm.globals, "eval", Native::Eval)?;
        let native_with_enter = self.native_value(Native::WithEnter);
        self.global(
            program,
            crate::bytecode::WITH_ENTER_BINDING,
            native_with_enter,
        )?;
        let native_with_exit = self.native_value(Native::WithExit);
        self.global(
            program,
            crate::bytecode::WITH_EXIT_BINDING,
            native_with_exit,
        )?;
        self.install_date(program)?;
        self.install_host_globals(program)?;
        self.install_errors(program)?;
        self.install_regexp(program)?;
        self.install_shadow_realm(program)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_date_regexp_realms");
        let symbol = self.native_value(Native::Symbol);
        self.set_builtin_function_name(symbol, "Symbol")?;
        self.install_native_rows(program, symbol, super::native_rows::Holder::SymbolConstructor)?;
        for well_known in WellKnownSymbol::ALL {
            let name = well_known.name();
            let value = self
                .heap
                .alloc_symbol(crate::heap::SymbolData::public(Some(format!("Symbol.{name}"))));
            self.well_known_symbols.insert(well_known, value);
            self.set_named_constant(program, symbol, name, value)?;
        }
        self.install_typed_array_iterator_symbol(program)?;
        let symbol_prototype_atom = self.prototype_atom();
        let symbol_prototype = self.get_property(program, symbol, symbol_prototype_atom)?;
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((self.realm.globals, Native::Symbol), symbol_prototype);
        let symbol_to_primitive = self.native_value(Native::SymbolToPrimitive);
        self.set_builtin_function_name(symbol_to_primitive, "[Symbol.toPrimitive]")?;
        let to_primitive = self.well_known_symbols[WellKnownSymbol::ToPrimitive];
        self.set_symbol_property(symbol_prototype, to_primitive, symbol_to_primitive)?;
        self.set_property_attributes(
            symbol_prototype,
            PropertyKey::symbol(to_primitive),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        let to_string_tag = self.well_known_symbols[WellKnownSymbol::ToStringTag];
        let symbol_tag = self.heap.alloc_string(JsString::from_str("Symbol"));
        self.set_symbol_property(symbol_prototype, to_string_tag, symbol_tag)?;
        self.set_property_attributes(
            symbol_prototype,
            PropertyKey::symbol(to_string_tag),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        let native_map = self.native_value(Native::Map);
        let native_set = self.native_value(Native::Set);
        self.install_collection_symbols(
            native_map,
            self.map_proto,
            native_set,
            self.set_proto,
            self.weak_map_proto,
            self.weak_set_proto,
        )?;
        self.install_shadow_realm_tag(self.shadow_realm_proto)?;
        let native_reg_exp = self.native_value(Native::RegExp);
        self.install_regexp_symbol_properties(
            native_reg_exp,
            self.regexp_proto,
            self.realm.globals,
        )?;
        let has_instance_symbol = self.well_known_symbols[WellKnownSymbol::HasInstance];
        let has_instance = self.native_value(Native::FunctionPrototypeHasInstance);
        self.set_builtin_function_name(has_instance, "[Symbol.hasInstance]")?;
        self.set_symbol_property(self.function_proto, has_instance_symbol, has_instance)?;
        self.set_property_attributes(
            self.function_proto,
            property_key::PropertyKey::symbol(has_instance_symbol),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.install_lazy_builtin_namespace(program, "FinalizationRegistry", Native::FinalizationRegistryInitialize)?;
        let bigint = self.native_value(Native::BigInt);
        let prototype_atom = self.prototype_atom();
        let bigint_prototype = self.get_property(program, bigint, prototype_atom)?;
        self.install_builtin_to_string_tag(bigint_prototype, "BigInt")?;
        let date = self.native_value(Native::Date);
        let date_prototype = self.get_property(program, date, prototype_atom)?;
        let date_to_primitive = self.native_value(Native::DateToPrimitive);
        self.set_builtin_function_name(date_to_primitive, "[Symbol.toPrimitive]")?;
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::ToPrimitive) {
            self.set_symbol_property(date_prototype, symbol, date_to_primitive)?;
            self.set_property_attributes(
                date_prototype,
                PropertyKey::symbol(symbol),
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
        self.install_array_species()?;
        let native_typed_array = self.native_value(Native::TypedArray);
        self.install_typed_array_species(
            native_typed_array,
            self.typed_array_proto,
            self.realm.globals,
        )?;
        self.install_array_unscopables(self.array_proto)?;
        self.install_abstract_module_source(program)?;
        self.install_array_buffer_species()?;
        self.global(program, "Symbol", symbol)?;
        let native_disposable_stack = self.native_value(Native::DisposableStack);
        let native_async_disposable_stack = self.native_value(Native::AsyncDisposableStack);
        self.install_disposable_stacks(
            program,
            self.realm.globals,
            self.object_proto,
            native_disposable_stack,
            native_async_disposable_stack,
        )?;
        let string = self.native_value(Native::String);
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_symbols_disposal");
        self.install_string(program, string)?;
        self.install_native_rows(program, string, super::native_rows::Holder::StringConstructor)?;
        self.global(program, "String", string)?;
        self.install_iterator_self(program)?;
        self.install_native_rows(program, self.realm.globals, super::native_rows::Holder::GlobalObject)?;
        self.install_number(program)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_core_builtins");
        self.install_lazy_builtin_namespace(program, "Intl", Native::IntlInitialize)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_intl");
        for (name, native) in [
            ("encodeURI", Native::EncodeUri),
            ("encodeURIComponent", Native::EncodeUriComponent),
            ("decodeURI", Native::DecodeUri),
            ("decodeURIComponent", Native::DecodeUriComponent),
        ] {
            self.set_builtin_named(program, self.realm.globals, name, native)?;
        }
        for (name, native) in [
            ("escape", Native::GlobalEscape),
            ("unescape", Native::GlobalUnescape),
        ] {
            self.set_builtin_named(program, self.realm.globals, name, native)?;
        }
        self.install_json_for_realm(program, self.realm.globals)?;
        self.install_lazy_builtin_namespace(program, "Reflect", Native::ReflectInitialize)?;
        self.install_lazy_builtin_namespace(program, "Math", Native::MathInitialize)?;
        self.install_promise(program)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_before_webassembly");
        self.install_lazy_builtin_namespace(program, "WebAssembly", Native::WebAssemblyInitialize)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_builtins_before_temporal");
        self.install_lazy_builtin_namespace(program, "Temporal", Native::TemporalInitialize)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_temporal");
        Ok(())
    }

    pub(super) fn install_lazy_builtin_namespace(
        &mut self,
        program: &ResidualProgram,
        name: &str,
        initializer: Native,
    ) -> Result<(), JsError> {
        let initializer = self.native_with_env(initializer, Value::NULL);
        let global = self.realm.globals;
        self.define_lazy_builtin_namespace(program, global, name, initializer)
    }

    /// Materializes the current realm's global `name` when it is still a pending lazy member,
    /// before a native reads an intrinsic that the global's initializer builds.
    pub(super) fn materialize_global_intrinsic(&mut self, program: &ResidualProgram, name: &str) -> Result<(), JsError> {
        let global = self.realm.globals;
        let atom = self.intern_atom(name);
        self.materialize_lazy_data_property(program, global, PropertyKey::string(atom))
    }

    /// Defines `holder.name` as a lazy namespace that the `initializer` function builds when
    /// first observed. A realm's own initializer is a native of that realm, so it builds the
    /// namespace in the realm owning `holder`.
    pub(super) fn define_lazy_builtin_namespace(
        &mut self,
        program: &ResidualProgram,
        holder: Value,
        name: &str,
        initializer: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        let key = self.heap.alloc_string(self.atom_value(atom));
        self.with_call_roots([holder, initializer, key], |vm| {
            vm.define_lazy_data_properties(
                program,
                holder,
                &[super::lazy_data_property::LazyMember {
                    key,
                    enumerable: false,
                    redefined: false,
                }],
                false,
                initializer,
            )
        })
    }

    fn install_prototypes(&mut self) {
        self.object_proto = self
            .heap
            .alloc_object(Self::empty_object(Value::NULL));
        self.function_proto = self.alloc_function_in(FunctionRecord {
            object: Self::empty_object(self.object_proto),
            kind: FunctionKind::Native(Native::FunctionPrototype),
            env: Value::NULL,
        }, self.realm.globals);
        for (native, prototype) in [
            (Native::Object, self.object_proto),
            (Native::Function, self.function_proto),
        ] {
            self.realm
                .intrinsics
                .builtin_prototypes
                .insert((self.realm.globals, native), prototype);
        }
        let object_proto = self.object_proto;
        self.object_data_mut(self.realm.globals)
            .unwrap()
            .set_proto(object_proto);
    }
    fn install_console(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let console = self.object();
        let native_print = self.native_value(Native::Print);
        self.set_named(program, console, "log", native_print)?;
        self.global(program, "console", console)
    }
    pub(super) fn install_json_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
    ) -> Result<(), JsError> {
        let prototype = self.realm_object_prototype(global);
        let json = self.heap.alloc_object(Self::empty_object(prototype));
        self.install_builtin_to_string_tag(json, "JSON")?;
        let realm = (global != self.realm.globals).then_some(global);
        self.install_rows_in_realm(program, json, super::native_rows::Holder::Json, realm)?;
        self.set_builtin_value_named(global, "JSON", json)
    }

    pub(super) fn install_reflect(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let reflect = self.object();
        self.install_native_rows(program, reflect, super::native_rows::Holder::Reflect)?;
        self.install_builtin_to_string_tag(reflect, "Reflect")?;
        self.seal_initialized_intrinsic(reflect);
        self.global(program, "Reflect", reflect)
    }
    pub(super) fn install_math(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let math = self.object();
        self.install_builtin_to_string_tag(math, "Math")?;
        // Functions, then value properties: the order Node's `Math` lists them in.
        for (name, native) in MATH_FUNCTIONS {
            self.set_builtin_named(program, math, name, *native)?;
        }
        for (name, value) in [
            ("E", std::f64::consts::E),
            ("LN10", std::f64::consts::LN_10),
            ("LN2", std::f64::consts::LN_2),
            ("LOG10E", std::f64::consts::LOG10_E),
            ("LOG2E", std::f64::consts::LOG2_E),
            ("PI", std::f64::consts::PI),
            ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
            ("SQRT2", std::f64::consts::SQRT_2),
        ] {
            self.set_named_constant(program, math, name, Value::number(value))?;
        }
        self.seal_initialized_intrinsic(math);
        self.global(program, "Math", math)
    }

    /// Installs `constructor[@@species]`, the accessor every species-reading constructor
    /// carries (configurable, not enumerable, no setter), whose getter `getter` makes; nothing
    /// where the realm has no @@species.
    pub(super) fn install_species(&mut self, constructor: Value, getter: impl FnOnce(&mut Self) -> Value) -> Result<(), JsError> {
        let Some(species) = self.well_known_symbols.get(WellKnownSymbol::Species) else { return Ok(()) };
        let getter = getter(self);
        self.set_builtin_function_name(getter, "get [Symbol.species]")?;
        self.set_symbol_property(constructor, species, Value::UNDEFINED)?;
        let attributes = PropertyAttributes {
            writable: false,
            enumerable: false,
            configurable: true,
            accessor: true,
            getter: Some(getter),
            setter: None,
        };
        self.set_property_attributes(constructor, PropertyKey::symbol(species), attributes);
        Ok(())
    }

    pub(super) fn install_builtin_to_string_tag(
        &mut self,
        object: Value,
        tag: &str,
    ) -> Result<(), JsError> {
        let symbol = self
            .well_known_symbols
            .get(WellKnownSymbol::ToStringTag)
            .ok_or_else(|| JsError("Symbol.toStringTag is not initialized".into()))?;
        let value = self.heap.alloc_string(tag.into());
        self.set_symbol_property(object, symbol, value)?;
        self.set_property_attributes(
            object,
            PropertyKey::symbol(symbol),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    pub(super) fn empty_object(proto: Value) -> Object {
        Object::new(proto)
    }
    /// The shared function of a `NATIVES` entry, built in the table's realm on first read.
    pub(super) fn native_value(&mut self, kind: Native) -> Value {
        if let Some(function) = self.natives.get(kind) {
            return function;
        }
        debug_assert!(
            super::native_rows::row_name(kind).is_some() || NATIVES.contains(&kind),
            "{kind:?} has no shared function"
        );
        let function = self.native_with_realm(kind, Value::NULL, self.natives.realm);
        self.natives.insert(kind, function);
        function
    }

    /// Whether `value` is the function every realm shares for `native` (`is_shared_native` of
    /// a function of that native): one comparison with the table's entry, no read of the
    /// function. The table enters only `native`'s own function under it (`native_value`).
    #[inline(always)]
    pub(super) fn is_shared_native_of(&self, value: Value, native: Native) -> bool {
        self.natives.get(native) == Some(value)
    }

    /// Whether `value` is the function every realm shares for its native: one native serves
    /// every realm, so it runs in the realm of its caller rather than switching to its own.
    pub(super) fn is_shared_native(&self, value: Value) -> bool {
        matches!(
            self.heap.function(value),
            Some(FunctionRecord { kind: FunctionKind::Native(kind), .. })
                if self.natives.get(*kind) == Some(value)
        )
    }
    pub(super) fn throw_type_error_for_current_realm(&mut self) -> Value {
        self.throw_type_error_for_realm(self.realm.globals)
    }
    pub(super) fn throw_type_error_for_realm(&mut self, global: Value) -> Value {
        match self
            .lookup_atom("\0quench:throw-type-error")
            .and_then(|atom| self.own_property(global, atom))
        {
            Some(thrower) => thrower,
            None => self.native_value(Native::ThrowTypeError),
        }
    }
    pub(super) fn install_throw_type_error_for_realm(
        &mut self,
        global: Value,
    ) -> Result<Value, JsError> {
        const INTRINSIC_KEY: &str = "\0quench:throw-type-error";
        let thrower = self.native_with_realm(Native::ThrowTypeError, global, global);
        self.set_builtin_function_name(thrower, "")?;
        for name in ["length", "name"] {
            let atom = self.intern_atom(name);
            self.set_property_attributes(
                thrower,
                PropertyKey::string(atom),
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
        self.object_data_mut(thrower)
            .expect("ThrowTypeError is a function object")
            .set_extensible(false);
        self.set_builtin_value_named(global, INTRINSIC_KEY, thrower)?;
        Ok(thrower)
    }
    pub(super) fn object(&mut self) -> Value {
        let prototype = self.realm_object_prototype(self.realm.globals);
        self.heap.alloc_empty_object(prototype)
    }
    pub(super) fn lookup_atom(&self, name: &str) -> Option<Atom> {
        self.find_atom(Self::atom_hash_str(name), |atom| {
            self.atom_units_equal_str(atom, name)
        })
    }
    /// The `prototype` atom, interned once per program reset instead of hashed per use.
    pub(super) fn prototype_atom(&mut self) -> Atom {
        if self.prototype_atom == NO_CACHED_ATOM {
            self.prototype_atom = self.intern_atom("prototype");
        }
        self.prototype_atom
    }

    /// `lookup_atom("prototype")` without hashing once the atom is cached.
    pub(super) fn known_prototype_atom(&self) -> Option<Atom> {
        if self.prototype_atom == NO_CACHED_ATOM {
            self.lookup_atom("prototype")
        } else {
            Some(self.prototype_atom)
        }
    }

    pub(super) fn intern_atom(&mut self, name: &str) -> Atom {
        #[cfg(feature = "profile-memory")]
        self.name_atom_memo.profile_query(name, |atom| self.atom_units_equal_str(atom, name));
        if let Some(atom) = self.name_atom_memo.candidate(name)
            && self.atom_units_equal_str(atom, name)
        {
            return atom;
        }
        let hash = Self::atom_hash_str(name);
        let atom = match self.find_atom(hash, |atom| self.atom_units_equal_str(atom, name)) {
            Some(atom) => atom,
            None => self.insert_dynamic_atom(JsString::from_str(name), hash),
        };
        self.name_atom_memo.record(name, atom);
        atom
    }
    pub(super) fn insert_dynamic_atom(&mut self, name: JsString, hash: u64) -> Atom {
        let atom = self.atom_sequence.len() as Atom;
        self.record_array_index_atom(name.host_string(), atom);
        self.atom_sequence.append(name);
        self.atom_classes.push(Default::default());
        self.index_atom_hash(hash, atom);
        self.profile.dynamic_atom();
        atom
    }
    /// Makes `atom`, spelled `name`, findable by its text and, for a canonical array index,
    /// by that index.
    pub(super) fn index_atom(&mut self, hash: u64, atom: Atom, name: &str) {
        self.record_array_index_atom(name, atom);
        self.index_atom_hash(hash, atom);
    }

    fn record_array_index_atom(&mut self, name: &str, atom: Atom) {
        if let Some(index) = super::object_static::array_index(name) {
            // The first atom with a text is the one text lookup finds.
            self.array_index_atoms.entry(index).or_insert(atom);
        }
    }

    fn index_atom_hash(&mut self, hash: u64, atom: Atom) {
        let (text, sequence) = (&self.atom_text, &self.atom_sequence);
        self.atoms.insert_unique(hash, atom, |&atom| {
            let index = atom as usize;
            if index < text.len() {
                Self::atom_hash_str(&text[index])
            } else {
                Self::atom_hash_units(sequence.appended(index - text.len()).units())
            }
        });
    }
    pub(super) fn atom_name(&self, atom: Atom) -> &str {
        let index = atom as usize;
        if index < self.atom_text.len() {
            &self.atom_text[index]
        } else {
            self.dynamic_atom(index).host_string()
        }
    }
    pub(super) fn global(
        &mut self,
        _p: &ResidualProgram,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        self.set_property_with_attributes(
            self.realm.globals,
            atom,
            value,
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        )?;
        Ok(())
    }
    pub(super) fn set_named(
        &mut self,
        _p: &ResidualProgram,
        object: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        self.set_property(object, atom, value)
    }
    pub(super) fn set_named_constant(
        &mut self,
        program: &ResidualProgram,
        object: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        self.set_named(program, object, name, value)?;
        let atom = self.intern_atom(name);
        self.set_property_attributes(
            object,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    pub(super) fn set_builtin_named(
        &mut self,
        _program: &ResidualProgram,
        object: Value,
        name: &str,
        native: Native,
    ) -> Result<(), JsError> {
        let function = self.native_value(native);
        self.set_builtin_function_value_named(object, name, function)
    }

    /// Installs `holder`'s rows on `target` as the shared builtin functions, in row order.
    pub(super) fn install_native_rows(
        &mut self,
        program: &ResidualProgram,
        target: Value,
        holder: super::native_rows::Holder,
    ) -> Result<(), JsError> {
        for row in super::native_rows::NATIVE_ROWS.iter().filter(|row| row.holder == holder) {
            self.set_builtin_named(program, target, row.name, row.native)?;
        }
        Ok(())
    }

    /// Installs `holder`'s rows on `target` as functions of `global`'s realm, in row order.
    pub(super) fn install_realm_native_rows(
        &mut self,
        target: Value,
        holder: super::native_rows::Holder,
        global: Value,
    ) -> Result<(), JsError> {
        for row in super::native_rows::NATIVE_ROWS.iter().filter(|row| row.holder == holder) {
            let function = self.native_with_realm(row.native, Value::NULL, global);
            self.set_builtin_function_name(function, row.name)?;
            self.set_builtin_value_named(target, row.name, function)?;
        }
        Ok(())
    }

    /// Installs `holder`'s rows on `target`: as the shared builtin functions when `realm` is `None`,
    /// else as functions of `realm`.
    pub(super) fn install_rows_in_realm(
        &mut self,
        program: &ResidualProgram,
        target: Value,
        holder: super::native_rows::Holder,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        match realm {
            None => self.install_native_rows(program, target, holder),
            Some(global) => self.install_realm_native_rows(target, holder, global),
        }
    }

    pub(super) fn set_builtin_function_value_named(
        &mut self,
        object: Value,
        name: &str,
        function: Value,
    ) -> Result<(), JsError> {
        self.set_builtin_function_name(function, name)?;
        self.set_builtin_value_named(object, name, function)
    }

    pub(super) fn set_builtin_function_name(
        &mut self,
        function: Value,
        name: &str,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom("name");
        let current = self.own_property(function, atom);
        if current.is_some_and(|value| {
            !self.heap.string(value).is_some_and(|text| text.units().is_empty())
        }) {
            return Ok(());
        }
        self.override_builtin_function_name(function, name)
    }

    pub(super) fn override_builtin_function_name(
        &mut self,
        function: Value,
        name: &str,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom("name");
        let property = PropertyKey::string(atom);
        // An existing `name` is made writable so the store below replaces it. A function
        // without one (a fresh bound function) gets it from the store; recording attributes
        // for an absent key would leave a stale descriptor and flush every field cache.
        if self.object_property_slot(function, property).is_some() {
            self.set_property_attributes(
                function,
                property,
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        // A builtin's name usually spells the key it is installed under, whose interned
        // string the name then shares instead of copying the text for every function.
        let name_atom = self.intern_atom(name);
        let value = self.heap.alloc_string(self.atom_value(name_atom));
        self.set_property(function, atom, value)?;
        self.set_property_attributes(
            function,
            property,
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        // A native function's source text derives from this name when requested
        // (`function_source_value`), so creating one records no source string.
        self.capture_function_name(function, value);
        Ok(())
    }

    pub(super) fn set_builtin_value_named(
        &mut self,
        object: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        self.set_property_with_attributes(
            object,
            atom,
            value,
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod math_function_tests {
    use super::*;

    #[test]
    fn math_functions_are_the_math_natives() {
        for (name, native) in MATH_FUNCTIONS {
            assert!(native.is_math_function(), "Math.{name}");
        }
        for native in (0..Native::COUNT).filter_map(Native::from_discriminant).filter(|native| native.is_math_function()) {
            assert!(
                MATH_FUNCTIONS.iter().any(|(_, listed)| *listed == native),
                "{native:?} is a Math function"
            );
        }
    }
}

/// The global object's value properties that are neither writable nor configurable (ECMA-262 19.1): no global
/// lexical declaration may shadow them (`HasRestrictedGlobalProperty`), so a read the global scope resolves is its constant.
pub(crate) fn immutable_globals() -> [(&'static str, Value); 3] {
    [("undefined", Value::UNDEFINED), ("NaN", Value::number(f64::NAN)), ("Infinity", Value::number(f64::INFINITY))]
}
