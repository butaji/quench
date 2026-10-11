use super::Runtime;
use crate::{Host, JsError, RootId, RootedError, Value, vm::Vm};

/// Opaque index into the embedding's stable native-operation table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostFunctionId(pub u32);

/// Iterator and cached next method, owned by its NativeContext's live roots.
pub struct RootedIterator {
    iterator: RootId,
    next: RootId,
}

/// Builtin internal-slot brands, independent of prototypes and guest property hooks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinBrand {
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
    ($( $scope:ident $name:literal ($length:literal) => $callback:path ),* $(,)?) => {
        &[$($crate::HostFunction {
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

    /// Test an internal-slot brand without invoking guest getters or proxy traps.
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

    /// Structured-clone a rooted value using the active realm's object graph.
    pub fn structured_clone_rooted(
        &mut self,
        value: RootId,
        options: Option<RootId>,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_structured_clone(value, options);
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
    #[cfg(feature = "profile-memory")]
    pub fn profile_memory_checkpoint(&self, phase: &str) {
        self.vm.report_memory_if_enabled(phase);
    }

    pub fn rooted_value(&self, root: RootId) -> Option<Value> {
        self.vm.root_value(root)
    }

    /// Copy the raw bytes from a typed array or DataView without per-index guest property reads.
    pub fn view_bytes_rooted(&self, root: RootId) -> Option<Vec<u8>> {
        self.vm
            .root_value(root)
            .and_then(|value| self.vm.embedding_view_bytes(value))
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

    pub fn get_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.get_property_rooted(object, key);
        self.completion(result)
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
}

#[cfg(test)]
#[path = "api_native_tests.rs"]
mod tests;
