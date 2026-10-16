use super::Runtime;
use crate::{Host, JsError, RootId, RootedError, Value, vm::Vm};

/// Opaque index into the embedding's stable native-operation table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostFunctionId(pub u32);

/// A host's synchronous module request never starts an async graph or re-enters
/// a currently evaluating module. Namespace roots use the native call's scope.
pub enum SynchronousModule<T> {
    Namespace(T),
    AsyncGraph,
    Evaluating,
}

/// Arithmetic operations at native boundaries use the same guest coercions as code.
#[derive(Clone, Copy)]
pub enum NativeArithmetic {
    Add,
    Multiply,
    Divide,
    BitwiseAnd,
}

/// Private weak host reference, pruned by the core collector before slot reuse.
/// Its persistent root must be released when the host resource is discarded.
#[derive(Clone, Copy, Debug)]
pub struct HostWeakRoot(pub(super) RootId);

/// Iterator and cached next method, owned by its NativeContext's live roots.
pub struct RootedIterator {
    iterator: RootId,
    next: RootId,
}

/// Builtin internal-slot brands, independent of prototypes and guest property hooks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinBrand {
    External,
    Date,
    RegExp,
    NativeError,
    Promise,
    Map,
    Set,
    WeakMap,
    WeakSet,
    Proxy,
    ArrayBuffer,
    SharedArrayBuffer,
    AnyArrayBuffer,
    DataView,
    TypedArray,
    ArrayBufferView,
    Uint8Array,
    Uint8ClampedArray,
    Uint16Array,
    Uint32Array,
    Int8Array,
    Int16Array,
    Int32Array,
    BigInt64Array,
    BigUint64Array,
    Float16Array,
    Float32Array,
    Float64Array,
    ArgumentsObject,
    AsyncFunction,
    GeneratorFunction,
    GeneratorObject,
    MapIterator,
    SetIterator,
    ModuleNamespaceObject,
    BooleanObject,
    NumberObject,
    StringObject,
    SymbolObject,
    BigIntObject,
    BoxedPrimitive,
}

/// A real Promise's current settlement, without running reactions or guest hooks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PromiseSnapshot<T = RootId> {
    Pending,
    Fulfilled(T),
    Rejected(T),
}

/// Flat native collection entries, preserving the current iterator position.
#[derive(Debug)]
pub struct CollectionPreview<T = RootId> {
    pub entries: Vec<T>,
    pub is_key_value: bool,
}

/// Intrinsic payload snapshot. Value roots remain scoped to the native callback.
#[derive(Debug)]
pub enum BuiltinData<T = RootId> {
    Primitive { brand: BuiltinBrand, value: T },
    Bytes { brand: BuiltinBrand, bytes: Vec<u8> },
    Map(Vec<(T, T)>),
    Set(Vec<T>),
    RegExp,
    Error,
    Opaque,
}

impl<T> BuiltinData<T> {
    pub(crate) fn map_values<U>(self, mut map: impl FnMut(T) -> U) -> BuiltinData<U> {
        match self {
            Self::Primitive { brand, value } => BuiltinData::Primitive {
                brand,
                value: map(value),
            },
            Self::Bytes { brand, bytes } => BuiltinData::Bytes { brand, bytes },
            Self::Map(entries) => BuiltinData::Map(
                entries
                    .into_iter()
                    .map(|(key, value)| (map(key), map(value)))
                    .collect(),
            ),
            Self::Set(entries) => BuiltinData::Set(entries.into_iter().map(map).collect()),
            Self::RegExp => BuiltinData::RegExp,
            Self::Error => BuiltinData::Error,
            Self::Opaque => BuiltinData::Opaque,
        }
    }
}

pub type HostCallback<H> =
    for<'a> fn(&mut NativeContext<'a, H>, RootId, &[RootId]) -> Result<RootId, RootedError>;

/// One authority for a native operation's descriptor and Rust implementation.
pub struct HostFunction<H: Host> {
    pub name: &'static str,
    pub length: u16,
    pub global: Option<&'static str>,
    pub call: HostCallback<H>,
}

/// Declare global and module-owned native descriptors from one binding table.
#[macro_export]
macro_rules! host_functions {
    ($( $(#[$attribute:meta])* $scope:ident $name:literal ($length:literal) => $callback:path ),* $(,)?) => {
        &[$($(#[$attribute])* $crate::HostFunction {
            name: $name,
            length: $length,
            global: $crate::host_functions!(@global $scope $name),
            call: $callback,
        }),*]
    };
    (@global global $name:literal) => { Some($name) };
    (@global method $name:literal) => { None };
}

/// Borrowed shared-VM access during a Rust native call. Roots created here
/// expire when the call returns. `retain` explicitly promotes a root for the
/// host to release later; fresh execution invalidates promoted roots too.
pub struct NativeContext<'a, H: Host> {
    vm: &'a mut Vm<H>,
    roots: Vec<RootId>,
}

impl<'a, H: Host> NativeContext<'a, H> {
    /// The core string representation's maximum UTF-16 code-unit count.
    pub const MAX_STRING_CODE_UNITS: usize = crate::vm::wtf16::MAX_STRING_UNITS;

    pub(crate) fn new(vm: &'a mut Vm<H>) -> Self {
        Self {
            vm,
            roots: Vec::new(),
        }
    }

    pub(crate) fn scoped_value(&mut self, value: Value) -> RootId {
        let root = self.vm.root(value);
        self.roots.push(root);
        root
    }

    fn error(&mut self, error: JsError) -> RootedError {
        let error = RootedError::retain(self.vm, error);
        self.roots.extend(error.exception);
        error
    }

    fn completion(&mut self, result: Result<Value, JsError>) -> Result<RootId, RootedError> {
        match result {
            Ok(value) => Ok(self.scoped_value(value)),
            Err(error) => Err(self.error(error)),
        }
    }

    /// Read string contents without coercion; `None` denotes a non-string.
    /// UTF-16 lone surrogates become replacement characters at this host-text boundary.
    pub fn string_text(&mut self, root: RootId) -> Result<Option<String>, RootedError> {
        self.vm
            .embedding_string_text(root)
            .map_err(|error| self.error(error))
    }

    /// Copy string code units without coercion or lone-surrogate loss.
    pub fn string_code_units(&mut self, root: RootId) -> Result<Option<Vec<u16>>, RootedError> {
        let result = self.vm.embedding_string_units(root);
        result.map_err(|error| self.error(error))
    }

    /// Apply the shared object integrity operation, including Proxy traps.
    pub fn prevent_extensions_rooted(&mut self, object: RootId) -> Result<bool, RootedError> {
        let result = self.vm.embedding_prevent_extensions(object);
        result.map_err(|error| self.error(error))
    }

    /// Apply [[HasProperty]], including prototype lookup and Proxy has traps.
    pub fn has_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
    ) -> Result<bool, RootedError> {
        self.vm
            .embedding_has_property(object, key)
            .map_err(|error| self.error(error))
    }

    /// Check whether a live rooted value is a JavaScript Symbol primitive.
    pub fn is_symbol_rooted(&mut self, root: RootId) -> Result<bool, RootedError> {
        let result = self.vm.embedding_is_symbol(root);
        result.map_err(|error| self.error(error))
    }

    /// Check whether a live rooted value has the ECMAScript Promise internal slots.
    pub fn is_promise_rooted(&mut self, root: RootId) -> Result<bool, RootedError> {
        let result = self.vm.embedding_is_promise(root);
        result.map_err(|error| self.error(error))
    }

    /// Observe settlement without then/species access, reactions or a job checkpoint.
    pub fn promise_snapshot_rooted(&mut self, promise: RootId) -> Result<Option<PromiseSnapshot>, RootedError> {
        let snapshot = self.vm.embedding_promise_snapshot(promise).map_err(|error| self.error(error))?;
        Ok(snapshot.map(|snapshot| match snapshot {
            PromiseSnapshot::Pending => PromiseSnapshot::Pending,
            PromiseSnapshot::Fulfilled(value) => PromiseSnapshot::Fulfilled(self.scoped_value(value)),
            PromiseSnapshot::Rejected(value) => PromiseSnapshot::Rejected(self.scoped_value(value)),
        }))
    }

    /// Mark the same rejection owner handled that then/await observe.
    pub fn mark_promise_handled_rooted(&mut self, promise: RootId) -> Result<(), RootedError> {
        self.vm.embedding_mark_promise_handled(promise).map_err(|error| self.error(error))
    }

    /// Whether a name was an own global property in the sealed host bootstrap.
    pub fn has_startup_global_name(&self, name: &str) -> bool {
        self.vm.has_startup_global_name(name)
    }

    /// A proxy's original target, including null for a revoked proxy, without traps.
    pub fn proxy_target_rooted(&mut self, root: RootId) -> Result<Option<RootId>, RootedError> {
        let result = self.vm.embedding_proxy_target(root);
        let value = result.map_err(|error| self.error(error))?;
        Ok(value.map(|value| self.scoped_value(value)))
    }

    /// Snapshot original target and handler without traps, including revoked nulls.
    pub fn proxy_details_rooted(&mut self, root: RootId) -> Result<Option<(RootId, RootId)>, RootedError> {
        let result = self.vm.embedding_proxy_details(root);
        let details = result.map_err(|error| self.error(error))?;
        Ok(details.map(|(target, handler)| (self.scoped_value(target), self.scoped_value(handler))))
    }

    /// The immutable display name of an ordinary function, without public getters.
    pub fn original_function_name_rooted(&mut self, root: RootId) -> Result<Option<RootId>, RootedError> {
        let result = self.vm.embedding_original_function_name(root);
        let name = result.map_err(|error| self.error(error))?;
        Ok(name.map(|name| self.string_units_rooted(&name.units().to_vec())))
    }

    /// Original base-constructor name, independent of mutable public properties.
    pub fn constructed_name_rooted(&mut self, root: RootId) -> Result<Option<RootId>, RootedError> {
        let result = self.vm.embedding_constructed_name(root);
        let name = result.map_err(|error| self.error(error))?;
        Ok(name.map(|name| self.string_units_rooted(&name.units().to_vec())))
    }

    /// Snapshot collection entries without advancing iterators or invoking hooks.
    pub fn collection_preview_rooted(&mut self, root: RootId) -> Result<Option<CollectionPreview>, RootedError> {
        let result = self.vm.embedding_collection_preview(root);
        let preview = result.map_err(|error| self.error(error))?;
        Ok(preview.map(|preview| CollectionPreview {
            entries: preview.entries.into_iter().map(|value| self.scoped_value(value)).collect(),
            is_key_value: preview.is_key_value,
        }))
    }

    /// Test an internal-slot brand without invoking guest getters or proxy traps.
    /// Create an opaque, non-owning native address wrapper. Core never dereferences
    /// the address; the embedding retains ownership of its native allocation.
    pub fn external_rooted(&mut self, address: usize) -> Result<RootId, RootedError> {
        let result = self.vm.create_embedding_external(address);
        self.completion(result)
    }

    pub fn external_address_rooted(&self, value: RootId) -> Option<usize> {
        self.vm.embedding_external_address(value)
    }

    pub fn has_builtin_brand_rooted(
        &mut self,
        root: RootId,
        brand: BuiltinBrand,
    ) -> Result<bool, RootedError> {
        let result = self.vm.embedding_has_builtin_brand(root, brand);
        result.map_err(|error| self.error(error))
    }

    /// Copy builtin internal data without invoking guest properties or proxy traps.
    pub fn builtin_data_rooted(
        &mut self,
        root: RootId,
    ) -> Result<Option<BuiltinData>, RootedError> {
        let result = self.vm.embedding_builtin_data(root);
        let data = result.map_err(|error| self.error(error))?;
        Ok(data.map(|data| data.map_values(|value| self.scoped_value(value))))
    }

    /// Read floating-point view elements through their internal slots.
    pub fn floating_view_values_rooted(
        &mut self,
        root: RootId,
    ) -> Result<Option<Vec<f64>>, RootedError> {
        let result = self.vm.embedding_floating_view_values(root);
        result.map_err(|error| self.error(error))
    }

    pub fn strict_equal_rooted(
        &mut self,
        left: RootId,
        right: RootId,
    ) -> Result<bool, RootedError> {
        let result = self.vm.embedding_strict_equal(left, right);
        result.map_err(|error| self.error(error))
    }

    /// Compiler-owned source names of live JavaScript callers, nearest first.
    /// This reads frame metadata without evaluating guest stack accessors.
    pub fn call_stack_source_names(&self) -> Vec<String> {
        self.vm.embedding_call_stack_source_names()
    }

    /// Apply shared InstanceofOperator, including original @@hasInstance hooks
    /// and proxy prototype effects.
    pub fn instance_of_rooted(&mut self, value: RootId, constructor: RootId) -> Result<bool, RootedError> {
        let result = self.vm.embedding_instanceof(value, constructor);
        result.map_err(|error| self.error(error))
    }

    /// Apply IsArray, including proxy target and revocation semantics.
    pub fn is_array_rooted(&mut self, root: RootId) -> Result<bool, RootedError> {
        let result = self.vm.embedding_is_array(root);
        result.map_err(|error| self.error(error))
    }

    /// Get an object's prototype through its internal method, preserving proxy effects.
    pub fn prototype_rooted(&mut self, root: RootId) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_prototype(root);
        self.completion(result)
    }

    /// Read a WeakMap entry through its internal slots, without guest hooks.
    pub fn weak_map_get_rooted(&mut self, map: RootId, key: RootId) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_weak_map_get(map, key);
        self.completion(result)
    }

    /// Reflect all own keys through the object's internal method.
    pub fn own_keys_rooted(&mut self, root: RootId) -> Result<Vec<RootId>, RootedError> {
        let result = self.vm.embedding_own_keys(root);
        let roots = result.map_err(|error| self.error(error))?;
        self.roots.extend(roots.iter().copied());
        Ok(roots)
    }

    /// Apply Object.defineProperty semantics to a rooted descriptor.
    pub fn define_property_descriptor_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        descriptor: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_define_descriptor(object, key, descriptor);
        self.completion(result)
    }

    /// Read an own property descriptor through the object's internal method.
    pub fn own_property_descriptor_rooted(
        &mut self,
        root: RootId,
        key: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_own_descriptor(root, key);
        self.completion(result)
    }

    /// Own non-array-index keys without expanding a view's virtual element keys.
    pub fn own_non_index_keys_rooted(&mut self, root: RootId, enumerable_only: bool) -> Result<Vec<RootId>, RootedError> {
        let result = self.vm.embedding_non_index_own_keys(root, enumerable_only);
        let roots = result.map_err(|error| self.error(error))?;
        self.roots.extend(roots.iter().copied());
        Ok(roots)
    }

    /// Enumerable own string keys, optionally including Symbols, through internal methods.
    pub fn enumerable_own_keys_rooted(
        &mut self,
        root: RootId,
        include_symbols: bool,
        include_indices: bool,
    ) -> Result<Vec<RootId>, RootedError> {
        let result = self
            .vm
            .embedding_enumerable_own_keys(root, include_symbols, include_indices);
        let roots = result.map_err(|error| self.error(error))?;
        self.roots.extend(roots.iter().copied());
        Ok(roots)
    }

    /// Apply the realm's ordinary ToString operation at a host boundary.
    pub fn to_string(&mut self, root: RootId) -> Result<String, RootedError> {
        let result = self.vm.embedding_to_string(root);
        result.map_err(|error| self.error(error))
    }

    /// Apply ordinary ToString while preserving every UTF-16 code unit.
    pub fn to_string_code_units(&mut self, root: RootId) -> Result<Vec<u16>, RootedError> {
        let result = self.vm.embedding_to_string_code_units(root);
        result.map_err(|error| self.error(error))
    }

    /// Apply shared ToNumber, including original number-hint conversion hooks
    /// and Symbol/BigInt rejection, without replacing it with host parsing.
    pub fn to_number(&mut self, root: RootId) -> Result<f64, RootedError> {
        let result = self.vm.embedding_to_number(root);
        result.map_err(|error| self.error(error))
    }

    pub fn arithmetic_rooted(
        &mut self,
        operation: NativeArithmetic,
        left: RootId,
        right: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_arithmetic(operation, left, right);
        self.completion(result)
    }

    /// Apply ToNumeric and the shared ++/-- numeric update, preserving BigInt.
    pub fn numeric_update_rooted(
        &mut self,
        value: RootId,
        decrement: bool,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_numeric_update(value, decrement);
        self.completion(result)
    }

    /// Apply ordinary subtraction, including guest conversion hooks and BigInt checks.
    pub fn subtract_rooted(&mut self, left: RootId, right: RootId) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_subtract(left, right);
        self.completion(result)
    }

    /// Apply the shared greater-than operation, including string/BigInt
    /// comparison and ordinary guest primitive-conversion hooks.
    pub fn greater_than_rooted(
        &mut self,
        left: RootId,
        right: RootId,
    ) -> Result<bool, RootedError> {
        let result = self.vm.embedding_greater_than(left, right);
        result.map_err(|error| self.error(error))
    }

    /// Apply shared >=, preserving conversion order, NaN and BigInt semantics.
    pub fn greater_equal_rooted(
        &mut self,
        left: RootId,
        right: RootId,
    ) -> Result<bool, RootedError> {
        let result = self.vm.embedding_greater_equal(left, right);
        result.map_err(|error| self.error(error))
    }

    /// Apply shared bitwise OR, preserving ToNumeric hooks and BigInt errors.
    pub fn bitwise_or_rooted(
        &mut self,
        left: RootId,
        right: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_bitwise_or(left, right);
        self.completion(result)
    }

    /// Acquire the shared VM iterator protocol and cache its next method once.
    pub fn iterator_rooted(&mut self, source: RootId) -> Result<RootedIterator, RootedError> {
        let result = self.vm.embedding_iterator_record(source);
        let (iterator, next) = result.map_err(|error| self.error(error))?;
        self.roots.extend([iterator, next]);
        Ok(RootedIterator { iterator, next })
    }

    pub fn iterator_step_rooted(
        &mut self,
        iterator: &RootedIterator,
    ) -> Result<Option<RootId>, RootedError> {
        let result = self
            .vm
            .embedding_iterator_step(iterator.iterator, iterator.next);
        let value = result.map_err(|error| self.error(error))?;
        Ok(value.map(|value| self.scoped_value(value)))
    }

    pub fn iterator_close_rooted(&mut self, iterator: &RootedIterator) -> Result<(), RootedError> {
        let result = self.vm.embedding_iterator_close(iterator.iterator);
        let value = self.completion(result)?;
        self.release_root(value);
        Ok(())
    }

    /// Apply ToBoolean without invoking guest coercion hooks.
    pub fn truthy_rooted(&mut self, root: RootId) -> Result<bool, RootedError> {
        self.vm
            .embedding_truthy(root)
            .map_err(|error| self.error(error))
    }

    /// Compare with SameValue: NaN equals itself and signed zeros differ.
    pub fn same_value_rooted(&mut self, left: RootId, right: RootId) -> Result<bool, RootedError> {
        self.vm
            .embedding_same_value(left, right)
            .map_err(|error| self.error(error))
    }

    /// Apply JavaScript abstract equality, preserving coercion effects and throws.
    pub fn equal_rooted(&mut self, left: RootId, right: RootId) -> Result<bool, RootedError> {
        self.vm
            .embedding_equal(left, right)
            .map_err(|error| self.error(error))
    }

    /// Evaluate named guest Script code in the active realm, without resetting roots.
    /// Caller lexical bindings and direct-eval grammar are not inherited.
    pub fn evaluate_script_rooted(
        &mut self,
        source: &str,
        name: &str,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.evaluate_embedding_script(source, name);
        self.completion(result)
    }

    /// Create a realm with fresh ECMAScript intrinsics and return its global object.
    pub fn create_realm_rooted(&mut self) -> Result<RootId, RootedError> {
        let result = self.vm.create_embedding_realm();
        self.completion(result)
    }

    /// Evaluate host Script source as a global Script of the realm owning `global`:
    /// its top-level lexical declarations persist in that realm.
    pub fn evaluate_script_in_realm_rooted(
        &mut self,
        global: RootId,
        source: &str,
        name: &str,
    ) -> Result<RootId, RootedError> {
        let result = self
            .vm
            .evaluate_embedding_script_in_realm(global, source, name);
        self.completion(result)
    }

    /// Compile and evaluate host-owned Script source through the specializer.
    /// Dynamic eval remains on `evaluate_script_rooted`.
    pub fn evaluate_specialized_script_rooted(
        &mut self,
        source: &str,
        name: &str,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.evaluate_embedding_specialized_script(source, name);
        self.completion(result)
    }

    /// Compile host-wrapped source with an explicit original-source origin.
    /// The offsets remove wrapper lines and its first-line UTF-16 columns from
    /// compiler-owned locations and diagnostic excerpts, not executable code.
    pub fn evaluate_specialized_script_with_origin_rooted(
        &mut self,
        source: &str,
        name: &str,
        line_offset: u32,
        column_offset: u32,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.evaluate_embedding_specialized_script_with_origin(
            source,
            name,
            Some((line_offset, column_offset)),
        );
        self.completion(result)
    }

    pub fn parse_json_rooted(&mut self, source: &str) -> Result<RootId, RootedError> {
        let result = self.vm.parse_embedding_json(source);
        self.completion(result)
    }

    /// Serialize through the intrinsic JSON operation, preserving toJSON,
    /// property access, cycles, BigInt failures and original guest exceptions.
    pub fn stringify_json_rooted(&mut self, value: RootId) -> Result<RootId, RootedError> {
        let result = self.vm.stringify_embedding_json(value);
        self.completion(result)
    }

    /// Structured-clone a rooted value using the active realm's object graph.
    /// Mark an ordinary host object as excluded from structured serialization.
    pub fn mark_uncloneable_rooted(&mut self, value: RootId) -> Result<(), RootedError> {
        self.vm.embedding_mark_uncloneable(value).map_err(|error| self.error(error))
    }

    pub fn mark_untransferable_rooted(&mut self, value: RootId) -> Result<(), RootedError> {
        self.vm
            .embedding_mark_untransferable(value)
            .map_err(|error| self.error(error))
    }

    pub fn is_marked_untransferable_rooted(&mut self, value: RootId) -> Result<bool, RootedError> {
        self.vm
            .embedding_is_marked_untransferable(value)
            .map_err(|error| self.error(error))
    }

    pub fn structured_clone_rooted(
        &mut self,
        value: RootId,
        options: Option<RootId>,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_structured_clone(value, options);
        self.completion(result)
    }

    /// Clone with an already-converted transfer sequence. Host Web IDL conversion
    /// owns iteration; the VM must not consult the public iterator a second time.
    pub fn structured_clone_with_transfers_rooted(
        &mut self,
        value: RootId,
        transfers: &[RootId],
    ) -> Result<RootId, RootedError> {
        let result = self
            .vm
            .embedding_structured_clone_with_transfers(value, transfers);
        self.completion(result)
    }

    pub fn array_rooted(&mut self, values: &[RootId]) -> Result<RootId, RootedError> {
        let result = self.vm.create_embedding_array(values);
        self.completion(result)
    }

    pub fn boolean(&mut self, value: bool) -> RootId {
        self.scoped_value(if value { Value::TRUE } else { Value::FALSE })
    }

    pub fn symbol_rooted(&mut self, description: Option<&str>) -> RootId {
        let description = description.map(str::to_owned);
        let symbol = self.vm.embedding_symbol(description);
        self.scoped_value(symbol)
    }

    pub fn null(&mut self) -> RootId {
        self.scoped_value(Value::NULL)
    }

    pub fn error_rooted(&mut self, message: &str) -> Result<RootId, RootedError> {
        let result = self
            .vm
            .create_embedding_exception(crate::heap::Native::Error, message);
        self.completion(result)
    }

    pub fn type_error_rooted(&mut self, message: &str) -> Result<RootId, RootedError> {
        let result = self
            .vm
            .create_embedding_exception(crate::heap::Native::TypeError, message);
        self.completion(result)
    }

    pub fn syntax_error_rooted(&mut self, message: &str) -> Result<RootId, RootedError> {
        let result = self
            .vm
            .create_embedding_exception(crate::heap::Native::SyntaxError, message);
        self.completion(result)
    }

    pub fn range_error_rooted(&mut self, message: &str) -> Result<RootId, RootedError> {
        let result = self
            .vm
            .create_embedding_exception(crate::heap::Native::RangeError, message);
        self.completion(result)
    }

    pub fn is_module(&mut self) -> Result<bool, RootedError> {
        self.vm
            .embedding_program()
            .map(|program| program.is_module())
            .map_err(|error| self.error(error))
    }

    /// Capture a guest value as private native environment data, traced by the VM.
    pub fn host_function_with_data(
        &mut self,
        id: HostFunctionId,
        data: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self
            .vm
            .embedding_value(data)
            .and_then(|value| self.vm.create_host_function_with_data(id, value));
        self.completion(result)
    }

    pub fn host_function_data(&mut self) -> Result<RootId, RootedError> {
        let result = self.vm.host_function_data();
        self.completion(result)
    }

    pub fn host_mut(&mut self) -> &mut H {
        &mut self.vm.host
    }

    /// Emit a memory census snapshot when this build enables `profile-memory`.
    /// Record the realm's intrinsics as they are now, before any script code runs.
    pub fn seal_startup_intrinsics(&mut self) {
        self.vm.seal_startup_intrinsics();
    }

    /// Capture an original data property from the sealed startup snapshot.
    /// Accessors, absent properties and expired holders return `None`.
    pub fn startup_data_property_rooted(&mut self, holder: RootId, name: &str) -> Option<RootId> {
        let holder = self.vm.root_value(holder)?;
        let value = self.vm.startup_data_property(holder, name)?;
        Some(self.scoped_value(value))
    }

    /// Run `build` with every sealed intrinsic at its startup descriptor, then give the
    /// script's own descriptors back, so builtin loading never observes a script's mutation.
    pub fn with_startup_intrinsics<T>(
        &mut self,
        build: impl FnOnce(&mut Self) -> Result<T, RootedError>,
    ) -> Result<T, RootedError> {
        let entered = self
            .vm
            .embedding_program()
            .and_then(|program| self.vm.enter_startup_intrinsics(&program));
        entered.map_err(|error| self.error(error))?;
        let built = build(self);
        let left = self
            .vm
            .embedding_program()
            .and_then(|program| self.vm.leave_startup_intrinsics(&program));
        let left = left.map_err(|error| self.error(error));
        let value = built?;
        left?;
        Ok(value)
    }

    #[cfg(feature = "profile-memory")]
    pub fn profile_memory_checkpoint(&self, phase: &str) {
        self.vm.report_memory_if_enabled(phase);
    }

    pub fn rooted_value(&self, root: RootId) -> Option<Value> {
        self.vm.root_value(root)
    }

    /// Copy native bytes into an intrinsic Uint8Array with its own fixed backing.
    /// No guest constructor, iterator, or indexed property hook is invoked.
    pub fn uint8_array_rooted(&mut self, bytes: &[u8]) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_uint8_array(bytes);
        self.completion(result)
    }

    /// Retain a private weak reference without ECMAScript kept-object effects.
    pub fn host_weak_root(&mut self, target: RootId) -> Result<HostWeakRoot, RootedError> {
        self.vm
            .embedding_host_weak_root(target)
            .map(HostWeakRoot)
            .map_err(|error| self.error(error))
    }

    /// Resolve a host weak reference into this callback's temporary root scope.
    pub fn resolve_host_weak_root(&mut self, reference: HostWeakRoot) -> Option<RootId> {
        self.vm
            .embedding_host_weak_value(reference.0)
            .map(|value| self.scoped_value(value))
    }

    pub fn release_host_weak_root(&mut self, reference: HostWeakRoot) -> bool {
        self.vm.release_root(reference.0)
    }

    /// Copy the raw bytes from a typed array or DataView without per-index guest property reads.
    pub fn view_bytes_rooted(&self, root: RootId) -> Option<Vec<u8>> {
        self.vm
            .root_value(root)
            .and_then(|value| self.vm.embedding_view_bytes(value))
    }

    /// Snapshot at most `maximum` original bytes of an ArrayBuffer or buffer view.
    pub fn bytes_prefix_rooted(&self, root: RootId, maximum: usize) -> Option<Vec<u8>> {
        self.vm.root_value(root).and_then(|value| self.vm.embedding_bytes_prefix(value, maximum))
    }

    /// Check callability without coercion or invoking guest code.
    pub fn is_callable_rooted(&mut self, root: RootId) -> Result<bool, RootedError> {
        self.vm
            .embedding_is_callable(root)
            .map_err(|error| self.error(error))
    }

    /// Check whether a rooted value has JavaScript `typeof value === "object"`.
    pub fn is_object_rooted(&mut self, root: RootId) -> Result<bool, RootedError> {
        self.vm
            .embedding_is_object(root)
            .map_err(|error| self.error(error))
    }

    pub fn number(&mut self, value: f64) -> RootId {
        self.scoped_value(Value::number(value))
    }

    /// Create an exact integer primitive without consulting the guest BigInt binding.
    pub fn bigint_rooted(&mut self, value: i128) -> RootId {
        let root = self.vm.bigint_rooted(value);
        self.roots.push(root);
        root
    }

    pub fn undefined(&mut self) -> RootId {
        self.scoped_value(Value::UNDEFINED)
    }

    pub fn string_rooted(&mut self, text: &str) -> RootId {
        let root = self.vm.string_rooted(text);
        self.roots.push(root);
        root
    }

    /// Create a JavaScript string from UTF-16 code units without Unicode-scalar loss.
    pub fn string_units_rooted(&mut self, units: &[u16]) -> RootId {
        let root = self.vm.string_units_rooted(units);
        self.roots.push(root);
        root
    }

    /// Create an ordinary object with the active realm's intrinsic prototype.
    pub fn object_rooted(&mut self) -> Result<RootId, RootedError> {
        let result = self.vm.create_embedding_object();
        self.completion(result)
    }

    /// Create a plain object whose `[[Prototype]]` is null.
    pub fn null_object_rooted(&mut self) -> Result<RootId, RootedError> {
        let result = self.vm.create_embedding_null_object();
        self.completion(result)
    }

    pub fn global_root(&mut self) -> Result<RootId, RootedError> {
        match self.vm.global_root() {
            Ok(root) => {
                self.roots.push(root);
                Ok(root)
            }
            Err(error) => Err(self.error(error)),
        }
    }

    pub fn host_function(&mut self, id: HostFunctionId) -> Result<RootId, RootedError> {
        let result = self.vm.create_host_function(id);
        self.completion(result)
    }

    /// Create a callback with an intrinsic JavaScript name independent of its
    /// private dispatch-table key, including unnamed native bindings.
    pub fn host_function_named(
        &mut self,
        id: HostFunctionId,
        name: &str,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.create_named_host_function(id, name);
        self.completion(result)
    }

    /// Create a named callback with private traced environment data.
    pub fn host_function_named_with_data(
        &mut self,
        id: HostFunctionId,
        name: &str,
        data: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_value(data)
            .and_then(|value| self.vm.create_named_host_function_with_data(id, name, value));
        self.completion(result)
    }

    /// Create a native-backed ordinary constructor. Primitive completions return
    /// the allocated receiver; object completions replace it, like JavaScript functions.
    pub fn host_constructor(&mut self, id: HostFunctionId) -> Result<RootId, RootedError> {
        let result = self.vm.create_host_constructor(id);
        self.completion(result)
    }

    pub fn host_constructor_with_data(
        &mut self,
        id: HostFunctionId,
        data: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_value(data)
            .and_then(|value| self.vm.create_host_constructor_with_data(id, value));
        self.completion(result)
    }

    pub fn evaluate_module_sync_rooted(
        &mut self,
        module: &crate::ModuleSource,
    ) -> Result<SynchronousModule<RootId>, RootedError> {
        let outcome = self.vm.evaluate_module_sync(module).map_err(|error| self.error(error))?;
        Ok(match outcome {
            SynchronousModule::Namespace(value) => SynchronousModule::Namespace(self.scoped_value(value)),
            SynchronousModule::AsyncGraph => SynchronousModule::AsyncGraph,
            SynchronousModule::Evaluating => SynchronousModule::Evaluating,
        })
    }

    /// Add a constant export while retaining a namespace's original live bindings.
    pub fn module_namespace_with_export_rooted(
        &mut self,
        namespace: RootId,
        name: &str,
        value: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.module_namespace_with_export_rooted(namespace, name, value);
        self.completion(result)
    }

    pub fn get_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.get_property_rooted(object, key);
        self.completion(result)
    }

    /// Perform the same strict named assignment as guest JavaScript.
    pub fn assign_named_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
        value: RootId,
    ) -> Result<(), RootedError> {
        self.vm.assign_named_property_rooted(object, name, value)
            .map_err(|error| self.error(error))
    }

    pub fn set_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        value: RootId,
        receiver: RootId,
    ) -> Result<bool, RootedError> {
        let result = self.vm.set_property_rooted(object, key, value, receiver);
        result.map_err(|error| self.error(error))
    }

    pub fn define_data_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        value: RootId,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<bool, RootedError> {
        self.vm
            .define_data_property_rooted(object, key, value, writable, enumerable, configurable)
            .map_err(|error| self.error(error))
    }

    /// Defines `names` on `holder` as one group of writable, configurable lazy data
    /// properties: the first time script observes any of them, `initializer` runs once, with
    /// the startup intrinsics in place, and defines them all.
    pub fn define_lazy_data_properties_rooted(
        &mut self,
        holder: RootId,
        names: &[&str],
        enumerable: bool,
        initializer: RootId,
    ) -> Result<(), RootedError> {
        self.vm
            .define_lazy_data_properties_rooted(holder, names, enumerable, initializer)
            .map_err(|error| self.error(error))
    }

    pub fn set_prototype_rooted(
        &mut self,
        object: RootId,
        prototype: RootId,
    ) -> Result<bool, RootedError> {
        self.vm
            .set_prototype_rooted(object, prototype)
            .map_err(|error| self.error(error))
    }

    pub fn call_rooted(
        &mut self,
        callee: RootId,
        receiver: RootId,
        args: &[RootId],
    ) -> Result<RootId, RootedError> {
        let result = self.vm.call_rooted(callee, receiver, args);
        self.completion(result)
    }

    /// Enqueue a callable in the realm's ECMAScript job queue.
    pub fn queue_microtask_rooted(
        &mut self,
        callback: RootId,
        args: &[RootId],
    ) -> Result<(), RootedError> {
        self.vm
            .embedding_enqueue_job(callback, args)
            .map_err(|error| self.error(error))
    }

    pub fn retain(&mut self, root: RootId) -> Result<RootId, RootedError> {
        let value = self
            .vm
            .embedding_value(root)
            .map_err(|error| self.error(error))?;
        Ok(self.vm.root(value))
    }

    /// Construct through the core; returned roots follow this callback's scope.
    pub fn construct_rooted(
        &mut self,
        callee: RootId,
        new_target: RootId,
        args: &[RootId],
    ) -> Result<RootId, RootedError> {
        let result = self.vm.construct_rooted(callee, new_target, args);
        self.completion(result)
    }

    pub fn release_root(&mut self, root: RootId) -> bool {
        self.vm.release_root(root)
    }

    pub fn collect(&mut self) -> Result<(), RootedError> {
        let program = self
            .vm
            .embedding_program()
            .map_err(|error| self.error(error))?;
        self.vm.collect_now(&program);
        Ok(())
    }

    pub fn throw(&mut self, exception: RootId) -> RootedError {
        match self.vm.embedding_value(exception) {
            Ok(value) => self.error(JsError::thrown(value, "host callback threw".into())),
            Err(error) => self.error(error),
        }
    }

    pub(crate) fn finish(&mut self, result: Result<RootId, RootedError>) -> Result<Value, JsError> {
        match result {
            Ok(root) => self.vm.embedding_value(root),
            Err(error) => Err(self.finish_error(error)),
        }
    }

    pub(crate) fn finish_error(&self, mut error: RootedError) -> JsError {
        match error.exception {
            Some(root) => match self.vm.embedding_value(root) {
                Ok(value) => {
                    error.error.replace_thrown_value(value);
                    error.error
                }
                Err(error) => error,
            },
            None if error.error.thrown_value().is_some() => JsError(error.error.to_string().into()),
            None => error.error,
        }
    }
}

impl<H: Host> Drop for NativeContext<'_, H> {
    fn drop(&mut self) {
        for root in &self.roots {
            self.vm.release_root(*root);
        }
    }
}

impl<H: Host> Runtime<H> {
    /// Create a persistent native callable from this host's stable table.
    pub fn host_function(&mut self, id: HostFunctionId) -> Result<RootId, RootedError> {
        match self.vm.create_host_function(id) {
            Ok(value) => Ok(self.root(value)),
            Err(error) => Err(RootedError::retain(&mut self.vm, error)),
        }
    }

    pub fn host_constructor(&mut self, id: HostFunctionId) -> Result<RootId, RootedError> {
        match self.vm.create_host_constructor(id) {
            Ok(value) => Ok(self.root(value)),
            Err(error) => Err(RootedError::retain(&mut self.vm, error)),
        }
    }
}

#[cfg(test)]
#[path = "api_native_tests.rs"]
mod tests;
