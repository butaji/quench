use super::Runtime;
use crate::{Host, JsError, RootId, RootedError, Value, vm::Vm};

/// Immutable host Script code compiled for one runtime state. It retains no
/// guest objects; each evaluation creates fresh runtime metadata and functions
/// in the chosen realm. A reset or another runtime invalidates this handle.
#[derive(Clone)]
pub struct PreparedScript {
    pub(crate) program: std::rc::Rc<crate::ResidualProgram>,
}

/// A logical host Script with one source identity and separate execution caches
/// in each realm. The handle owns code, not guest objects or realm globals.
#[derive(Clone)]
pub struct CompiledScript {
    pub(crate) state: std::rc::Rc<CompiledScriptState>,
}

pub(crate) struct CompiledScriptState {
    pub(crate) program: std::rc::Rc<crate::ResidualProgram>,
    pub(crate) source: crate::vm::ProgramId,
}

/// An embedder's hooks for its scripted serializer (V8's `ValueSerializer::Delegate`).
pub trait SerializerDelegate<H: Host> {
    /// Write a host object's contents; its tag is already written. A returned value is
    /// written next, through the same serializer.
    fn write_host_object(
        &mut self,
        context: &mut NativeContext<'_, H>,
        output: &crate::WireOutput,
        object: RootId,
    ) -> Result<Option<RootId>, RootedError>;
    fn shared_array_buffer_id(&mut self, context: &mut NativeContext<'_, H>, buffer: RootId) -> Result<u32, RootedError>;
    /// The exception for a value that cannot be cloned, already thrown.
    fn data_clone_error(&mut self, context: &mut NativeContext<'_, H>, message: &str) -> RootedError;
}

/// An embedder's hooks for its scripted deserializer (V8's `ValueDeserializer::Delegate`).
pub trait DeserializerDelegate<H: Host> {
    fn read_host_object(
        &mut self,
        context: &mut NativeContext<'_, H>,
        input: &crate::SharedWireInput,
    ) -> Result<crate::HostObjectRead<RootId>, RootedError>;
    fn shared_array_buffer(&mut self, context: &mut NativeContext<'_, H>, id: u32) -> Result<RootId, RootedError>;
}

/// Embedder delegates seen as the serializer's internal hooks: each call runs in a
/// native scope over the VM.
struct Scripted<'d, D: ?Sized>(&'d mut D);

fn hook_value<H: Host>(context: &mut NativeContext<'_, H>, result: Result<RootId, RootedError>) -> Result<Value, JsError> {
    result
        .map(|root| context.vm.root_value(root).unwrap_or(Value::UNDEFINED))
        .map_err(|error| error.error)
}

impl<H: Host, D: SerializerDelegate<H> + ?Sized> crate::vm::WriteHooks<H> for Scripted<'_, D> {
    fn write_host_object(&mut self, vm: &mut Vm<H>, _: &crate::ResidualProgram, output: &crate::WireOutput, object: Value) -> Result<Option<Value>, JsError> {
        let mut context = NativeContext::new(vm);
        let object = context.scoped_value(object);
        match self.0.write_host_object(&mut context, output, object) {
            Ok(Some(next)) => hook_value(&mut context, Ok(next)).map(Some),
            Ok(None) => Ok(None),
            Err(error) => Err(error.error),
        }
    }

    fn shared_array_buffer_id(&mut self, vm: &mut Vm<H>, _: &crate::ResidualProgram, buffer: Value) -> Result<u32, JsError> {
        let mut context = NativeContext::new(vm);
        let buffer = context.scoped_value(buffer);
        self.0.shared_array_buffer_id(&mut context, buffer).map_err(|error| error.error)
    }

    fn data_clone_error(&mut self, vm: &mut Vm<H>, _: &crate::ResidualProgram, message: &str) -> JsError {
        let mut context = NativeContext::new(vm);
        self.0.data_clone_error(&mut context, message).error
    }
}

impl<H: Host, D: DeserializerDelegate<H> + ?Sized> crate::vm::ReadHooks<H> for Scripted<'_, D> {
    fn read_host_object(
        &mut self,
        vm: &mut Vm<H>,
        _: &crate::ResidualProgram,
        input: &crate::SharedWireInput,
    ) -> Result<crate::HostObjectRead<Value>, JsError> {
        let mut context = NativeContext::new(vm);
        match self.0.read_host_object(&mut context, input) {
            Ok(crate::HostObjectRead::Object(object)) => hook_value(&mut context, Ok(object)).map(crate::HostObjectRead::Object),
            Ok(crate::HostObjectRead::NextValue) => Ok(crate::HostObjectRead::NextValue),
            Err(error) => Err(error.error),
        }
    }

    fn shared_array_buffer(&mut self, vm: &mut Vm<H>, _: &crate::ResidualProgram, id: u32) -> Result<Value, JsError> {
        let mut context = NativeContext::new(vm);
        let result = self.0.shared_array_buffer(&mut context, id);
        hook_value(&mut context, result)
    }
}

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

/// A copied projection of a genuine JavaScript activation. Native helper frames are excluded.
pub struct SourceStackFrame {
    /// UTF-16 code units preserve function names, including lone surrogates.
    pub function_name: Vec<u16>,
    /// Positive, non-reused compiled-source identity; zero is reserved for no script.
    pub script_id: u64,
    pub script_name: String,
    /// True when the activation belongs to an unnamed eval Script.
    pub is_eval: bool,
    pub line_number: u32,
    pub column_number: u32,
}

/// Compiler-owned source for an unformatted captured error, or the current activation.
/// Positions are one-based, as in source stack frames; hosts choose their API projection.
pub struct ErrorSourcePosition {
    pub source_line: Option<String>,
    pub script_name: Option<String>,
    pub line_number: u32,
    pub column_number: u32,
}

/// Private weak host reference, pruned by the core collector before slot reuse.
/// Its persistent root must be released when the host resource is discarded.
#[derive(Clone, Copy, Debug)]
pub struct HostWeakRoot(pub(crate) RootId);

/// Iterator and cached next method, owned by its NativeContext's live roots.
pub struct RootedIterator {
    iterator: RootId,
    next: RootId,
}

/// An ECMAScript language type; `Function` marks callable objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    Undefined,
    Null,
    Boolean,
    Number,
    String,
    Symbol,
    BigInt,
    Object,
    Function,
}

/// The bytes an ArrayBuffer or view names. `kind` is the view's specific brand
/// (`ArrayBuffer`, `SharedArrayBuffer`, `DataView` or a typed-array constructor brand).
#[derive(Clone, Copy, Debug)]
pub struct BufferStorage {
    pub kind: BuiltinBrand,
    /// The first byte of the view (the buffer's base plus `byte_offset`); null when empty.
    pub data: *mut u8,
    pub byte_offset: usize,
    pub byte_length: usize,
    /// Element count for a typed array, otherwise the byte length.
    pub length: usize,
    pub buffer: RootId,
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

/// A property descriptor's fields, as [[GetOwnProperty]] reports them or [[DefineOwnProperty]]
/// reads them, without a descriptor object. An absent field is `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DescriptorFields<T = RootId> {
    pub value: Option<T>,
    pub writable: Option<bool>,
    pub get: Option<T>,
    pub set: Option<T>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
}

impl<T> Default for DescriptorFields<T> {
    fn default() -> Self {
        Self { value: None, writable: None, get: None, set: None, enumerable: None, configurable: None }
    }
}

impl<T> DescriptorFields<T> {
    fn try_map_values<U, E>(self, mut map: impl FnMut(T) -> Result<U, E>) -> Result<DescriptorFields<U>, E> {
        Ok(DescriptorFields {
            value: self.value.map(&mut map).transpose()?,
            writable: self.writable,
            get: self.get.map(&mut map).transpose()?,
            set: self.set.map(&mut map).transpose()?,
            enumerable: self.enumerable,
            configurable: self.configurable,
        })
    }
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
    /// The listed roots this scope releases one by one when it ends.
    roots: Vec<RootId>,
    /// Where this scope's scoped roots (`scoped_value`) begin on the root table's stack,
    /// which its end truncates to.
    scoped_height: usize,
    /// `new.target` when this context runs a host constructor's [[Construct]].
    new_target: Option<Value>,
}

/// Where a native scope's roots begin (`NativeContext::root_scope_mark`): the roots made
/// after it, listed or scoped, are a nested scope's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RootScopeMark {
    listed: usize,
    scoped: usize,
}

impl<'a, H: Host> NativeContext<'a, H> {
    /// Count actual heap record/arena capacities and unique live payloads without guest effects.
    /// An owned cancellation handle for a native operation; no Vm reference escapes.
    pub fn execution_interrupt_handle(&self)->crate::ExecutionInterrupt { self.vm.execution_interrupt_handle() }
    /// Register an idempotent native wakeup while a host operation is blocked.
    /// Keep the strong callback until the wait ends; the Core stores only a Weak.
    pub fn register_termination_waker(&self,wake:&std::sync::Arc<dyn Fn()+Send+Sync>) {
        self.vm.execution_interrupt_handle().register_termination_waker(wake)
    }
    /// Propagate host cancellation as the same uncatchable completion as bytecode.
    pub fn check_execution_interrupt(&mut self)->Result<(),RootedError> {
        self.vm.check_execution_interrupt().map_err(|error|self.error(error))
    }
    pub fn heap_memory_composition(&self) -> crate::HeapMemoryComposition {
        self.vm.embedding_heap_memory_composition()
    }

    /// The core string representation's maximum UTF-16 code-unit count.
    pub const MAX_STRING_CODE_UNITS: usize = crate::vm::wtf16::MAX_STRING_UNITS;

    pub(crate) fn new(vm: &'a mut Vm<H>) -> Self {
        Self::with_new_target(vm, None)
    }

    /// A callback's context. Its root list comes from the pool `Drop` returns lists to, so
    /// the pool stays as deep as the native calls nested at once.
    pub(crate) fn with_new_target(vm: &'a mut Vm<H>, new_target: Option<Value>) -> Self {
        let roots = vm.native_root_lists.pop().unwrap_or_default();
        let scoped_height = vm.scoped_root_height();
        Self {
            vm,
            roots,
            scoped_height,
            new_target,
        }
    }

    /// A root in this callback's scope naming the same value as `root`, which may be a
    /// promoted root the host releases separately.
    pub fn scope_root(&mut self, root: RootId) -> Option<RootId> {
        self.vm.root_value(root).map(|value| self.scoped_value(value))
    }

    /// Classify a rooted value without coercion or guest effects.
    pub fn value_type_rooted(&self, root: RootId) -> Option<ValueType> {
        self.vm.root_value(root).map(|value| self.vm.value_type(value))
    }

    /// `new.target` of the running host callback: `Some` only while it is constructed.
    pub fn new_target_rooted(&mut self) -> Option<RootId> {
        self.new_target.map(|value| self.scoped_value(value))
    }

    /// Where the roots this callback scope makes next begin; a later
    /// [`release_roots_since`](Self::release_roots_since) closes a nested scope at it.
    pub fn root_scope_mark(&self) -> RootScopeMark {
        RootScopeMark { listed: self.roots.len(), scoped: self.vm.scoped_root_height() }
    }

    /// Release the roots created after `mark`, except `keep`, which move to the
    /// enclosing scope (an escaped handle).
    pub fn release_roots_since(&mut self, mark: RootScopeMark, keep: &[RootId]) {
        self.vm.release_root_scope_keeping(mark.scoped, keep);
        if mark.listed >= self.roots.len() {
            return;
        }
        let created = self.roots.split_off(mark.listed);
        for root in created {
            if keep.contains(&root) {
                self.roots.push(root);
            } else {
                self.vm.release_root(root);
            }
        }
    }

    /// The pinned bytes of an ArrayBuffer, typed array or DataView. A fixed buffer keeps
    /// this address for its lifetime, so native code may hold the pointer while the
    /// buffer is reachable; a resizable buffer keeps it only until it resizes.
    pub fn buffer_storage_rooted(
        &mut self,
        root: RootId,
    ) -> Result<Option<BufferStorage>, RootedError> {
        let result = self.vm.embedding_buffer_storage(root);
        let storage = result.map_err(|error| self.error(error))?;
        Ok(storage.map(|(storage, buffer)| BufferStorage {
            buffer: self.scoped_value(buffer),
            ..storage
        }))
    }

    pub(crate) fn scoped_value(&mut self, value: Value) -> RootId {
        self.vm.root_scoped(value)
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

    /// The length of a string's UTF-8 encoding without coercion or copying, a lone surrogate
    /// counted as the replacement character; `None` denotes a non-string.
    pub fn string_utf8_length(&mut self, root: RootId) -> Result<Option<usize>, RootedError> {
        let result = self.vm.embedding_string_utf8_length(root);
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

    /// Allocate an intrinsic pending Promise without invoking guest constructors.
    /// The returned root belongs to this callback frame; retain it for later completion.
    pub fn pending_promise_rooted(&mut self) -> Result<RootId, RootedError> {
        let value = self.vm.embedding_pending_promise().map_err(|error| self.error(error))?;
        Ok(self.scoped_value(value))
    }

    /// Resolve a genuine Promise through the intrinsic thenable assimilation algorithm.
    /// A settled Promise is unchanged; no public constructor, resolve or then method is used.
    pub fn resolve_promise_rooted(&mut self, promise: RootId, value: RootId) -> Result<(), RootedError> {
        self.vm.embedding_resolve_promise(promise, value).map_err(|error| self.error(error))
    }

    /// Reject a genuine Promise with the exact rooted reason, including undefined.
    pub fn reject_promise_rooted(&mut self, promise: RootId, reason: RootId) -> Result<(), RootedError> {
        self.vm.embedding_reject_promise(promise, reason).map_err(|error| self.error(error))
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

    /// Constructor display from creation identity and safe prototype data.
    pub fn constructor_name_rooted(&mut self, root: RootId) -> Result<Option<RootId>, RootedError> {
        let result = self.vm.embedding_constructor_name(root);
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

    /// Capture actual JavaScript source frames independently of Error hooks and stack limits.
    pub fn source_stack_frames(&mut self, limit: usize) -> Vec<SourceStackFrame> {
        self.vm.embedding_source_stack_frames(limit)
    }

    /// Read captured source without invoking properties, getters or proxy traps.
    /// With no unformatted capture, project the current genuine source activation.
    pub fn error_source_position_rooted(
        &mut self,
        error: RootId,
    ) -> Result<Option<ErrorSourcePosition>, RootedError> {
        let result = self.vm.embedding_error_source_position(error);
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

    /// CreateListFromArrayLike: the values `length` and the indices below it give, read as
    /// guest code reads them.
    pub fn array_like_values_rooted(&mut self, root: RootId) -> Result<Vec<RootId>, RootedError> {
        let result = self.vm.embedding_array_like_values(root);
        let roots = result.map_err(|error| self.error(error))?;
        self.roots.extend(roots.iter().copied());
        Ok(roots)
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

    /// [[GetOwnProperty]]: the own property `key` of `object` as descriptor fields, with
    /// no descriptor object; `None` when there is none.
    pub fn own_property_fields_rooted(
        &mut self,
        object: RootId,
        key: RootId,
    ) -> Result<Option<DescriptorFields>, RootedError> {
        let result = self.vm.embedding_own_property_fields(object, key);
        let fields = result.map_err(|error| self.error(error))?;
        fields
            .map(|fields| fields.try_map_values(|value| Ok(self.scoped_value(value))))
            .transpose()
    }

    /// `Reflect.defineProperty` with a descriptor given as its fields: whether the
    /// definition succeeded.
    pub fn define_property_fields_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        fields: DescriptorFields,
    ) -> Result<bool, RootedError> {
        let result = fields
            .try_map_values(|root| self.vm.embedding_value(root))
            .and_then(|fields| self.vm.embedding_define_property_fields(object, key, fields));
        result.map_err(|error| self.error(error))
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

    /// Whether two values are provably the same plain data (`Vm::plain_data_equal`): the same
    /// value, or dense arrays and ordinary objects of the intrinsic prototypes, holding only
    /// string-keyed data properties, with the same enumerable keys and pairwise the same
    /// plain data. Observes nothing; `false` proves nothing.
    pub fn plain_data_equal_rooted(&mut self, left: RootId, right: RootId) -> Result<bool, RootedError> {
        self.vm
            .embedding_plain_data_equal(left, right)
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

    /// Compile a host wrapper in its target realm, retaining original-source positions.
    pub fn evaluate_specialized_script_in_realm_with_origin_rooted(
        &mut self, global: RootId, source: &str, name: &str,
        line_offset: u32, column_offset: u32,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.evaluate_embedding_specialized_script_in_realm_with_origin(
            global, source, name, (line_offset, column_offset),
        );
        self.completion(result)
    }

    /// Compile host Script code once, preserving its original source positions.
    pub fn prepare_specialized_script_in_realm_with_origin(
        &mut self, global: RootId, source: &str, name: &str, line_offset: u32, column_offset: u32,
    ) -> Result<PreparedScript, RootedError> {
        self.vm.prepare_embedding_specialized_script_in_realm_with_origin(
            global, source, name, (line_offset, column_offset),
        ).map_err(|error| self.error(error))
    }

    /// Compile one logical Script. Its source identity persists across runs;
    /// realm-bound execution state is allocated independently on first use.
    pub fn compile_script_in_realm_with_origin(
        &mut self, global: RootId, source: &str, name: &str,
        line_offset: u32, column_offset: u32,
    ) -> Result<CompiledScript, RootedError> {
        self.vm.compile_embedding_script_in_realm(
            global, source, name, (line_offset, column_offset),
        ).map_err(|error| self.error(error))
    }

    pub fn compiled_script_is_current(&self, script: &CompiledScript) -> bool {
        self.vm.compiled_script_is_current(script)
    }

    pub fn evaluate_compiled_script_in_realm_rooted(
        &mut self, global: RootId, script: &CompiledScript,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.evaluate_embedding_compiled_script_in_realm(global, script);
        self.completion(result)
    }

    pub fn prepared_script_is_current(&self, prepared: &PreparedScript) -> bool {
        self.vm.prepared_script_is_current(prepared)
    }

    /// Evaluate shared code with fresh execution state and the target's intrinsics.
    pub fn evaluate_prepared_script_in_realm_rooted(
        &mut self, global: RootId, prepared: &PreparedScript,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.evaluate_embedding_prepared_script_in_realm(global, prepared);
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

    /// `evaluate_specialized_script_with_origin_rooted` for an application's entry, whose
    /// functions keep their compiled records instead of compiling again on first call.
    pub fn evaluate_entry_script_with_origin_rooted(
        &mut self,
        source: &str,
        name: &str,
        line_offset: u32,
        column_offset: u32,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.evaluate_embedding_entry_script_with_origin(
            source,
            name,
            Some((line_offset, column_offset)),
        );
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

    /// Compile and evaluate a host wrapper `prefix + body + suffix` whose body is static
    /// host text (an embedded library module): the program retains the body instead of a
    /// copy of the wrapped source, and positions start at the wrapper origin. The compiled
    /// program goes through the host's compile cache (`Host::kept_compiled_program`).
    pub fn evaluate_specialized_static_body_rooted(
        &mut self,
        prefix: &str,
        body: &'static str,
        suffix: &str,
        name: &str,
    ) -> Result<RootId, RootedError> {
        self.evaluate_library_body(prefix, crate::bytecode::SourceText::Static(body), suffix, name)
    }

    /// `evaluate_specialized_static_body_rooted` for a library module text the host derived
    /// from its embedded text (a projection), which the program retains instead.
    pub fn evaluate_specialized_library_body_rooted(
        &mut self,
        prefix: &str,
        body: String,
        suffix: &str,
        name: &str,
    ) -> Result<RootId, RootedError> {
        let body = crate::bytecode::SourceText::Shared(std::sync::Arc::from(body));
        self.evaluate_library_body(prefix, body, suffix, name)
    }

    fn evaluate_library_body(
        &mut self,
        prefix: &str,
        body: crate::bytecode::SourceText,
        suffix: &str,
        name: &str,
    ) -> Result<RootId, RootedError> {
        let result = self
            .vm
            .prepare_embedding_library_body_kept(prefix, body, suffix, name)
            .and_then(|prepared| self.vm.evaluate_embedding_prepared_script(&prepared));
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

    /// How V8 shows a value in a DataCloneError, without side effects.
    pub fn clone_description_rooted(&mut self, value: RootId) -> String {
        match self.vm.root_value(value) {
            Some(value) => self.vm.clone_description(value),
            None => String::new(),
        }
    }

    /// `ValueSerializer::WriteValue` through an embedder's hooks.
    pub fn serializer_write_value(
        &mut self,
        serializer: &mut crate::ValueSerializer,
        value: RootId,
        delegate: &mut dyn SerializerDelegate<H>,
    ) -> Result<(), RootedError> {
        let result = self.vm.embedding_value(value).and_then(|value| {
            let program = self.vm.embedding_program()?;
            self.vm.wire_write(&program, serializer, &mut Scripted(delegate), value)
        });
        result.map_err(|error| self.error(error))
    }

    /// `ValueSerializer::TransferArrayBuffer`; `false` when `buffer` is no ArrayBuffer.
    pub fn serializer_transfer_array_buffer(
        &mut self,
        serializer: &mut crate::ValueSerializer,
        id: u32,
        buffer: RootId,
    ) -> Result<bool, RootedError> {
        let buffer = self.vm.embedding_value(buffer).map_err(|error| self.error(error))?;
        if !self.vm.is_unshared_array_buffer(buffer) {
            return Ok(false);
        }
        self.vm.wire_transfer_array_buffer(serializer, id, buffer);
        Ok(true)
    }

    /// Release the objects a serializer numbered.
    pub fn serializer_release(&mut self, serializer: &mut crate::ValueSerializer) {
        self.vm.release_wire_serializer(serializer);
    }

    /// `ValueDeserializer::ReadHeader`.
    pub fn deserializer_read_header(&mut self, deserializer: &mut crate::ValueDeserializer) -> Result<(), RootedError> {
        let result = self.vm.embedding_program().and_then(|program| self.vm.wire_read_header(&program, deserializer));
        result.map_err(|error| self.error(error))
    }

    /// `ValueDeserializer::ReadValue` through an embedder's hooks.
    pub fn deserializer_read_value(
        &mut self,
        deserializer: &mut crate::ValueDeserializer,
        delegate: &mut dyn DeserializerDelegate<H>,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_program().and_then(|program| {
            self.vm.wire_read_value(&program, deserializer, &mut Scripted(delegate))
        });
        self.completion(result)
    }

    /// `ValueDeserializer::TransferArrayBuffer`; `false` when `buffer` is no
    /// ArrayBuffer or SharedArrayBuffer.
    pub fn deserializer_transfer_array_buffer(
        &mut self,
        deserializer: &mut crate::ValueDeserializer,
        id: u32,
        buffer: RootId,
    ) -> Result<bool, RootedError> {
        let buffer = self.vm.embedding_value(buffer).map_err(|error| self.error(error))?;
        if !self.vm.is_array_buffer(buffer) {
            return Ok(false);
        }
        self.vm.wire_receive_array_buffer(deserializer, id, buffer);
        Ok(true)
    }

    /// Release the objects a deserializer created or was handed.
    pub fn deserializer_release(&mut self, deserializer: &mut crate::ValueDeserializer) {
        self.vm.release_wire_deserializer(deserializer);
    }

    /// Capture a Send + Sync graph; source transfer detachment occurs once here.
    pub fn capture_owned_clone_rooted(&mut self,value:RootId,transfers:&[RootId])->Result<crate::owned_clone::Graph,RootedError>{crate::owned_clone::capture(self,value,transfers)}
    pub fn import_owned_clone_rooted(&mut self,graph:&crate::owned_clone::Graph)->Result<RootId,RootedError>{crate::owned_clone::import(self,graph)}
    pub fn clone_metadata_rooted(&mut self,value:RootId)->Result<Option<crate::owned_clone::Metadata>,RootedError>{
        use crate::owned_clone::Metadata as M;
        let metadata=self.vm.embedding_clone_metadata(value).map_err(|e|self.error(e))?;
        Ok(metadata.map(|m|match m{
            M::View{kind,buffer,offset,length,tracking}=>M::View{kind,buffer:self.scoped_value(buffer),offset,length,tracking},
            M::SharedBuffer(b)=>M::SharedBuffer(b),M::RegExp(s,f)=>M::RegExp(s,f),M::Error(s)=>M::Error(s),
            M::Buffer{bytes,shared,detached,maximum,resizable}=>M::Buffer{bytes,shared,detached,maximum,resizable},
        }))
    }
    pub fn clone_bigint_rooted(&mut self,text:&str)->Result<RootId,RootedError>{let result=self.vm.embedding_clone_bigint(text);self.completion(result)}
    pub fn clone_allocate_rooted(&mut self,node:&crate::owned_clone::Node,operands:&[RootId])->Result<RootId,RootedError>{let result=self.vm.embedding_clone_allocate(node,operands);self.completion(result)}
    pub fn clone_collection_insert_rooted(&mut self,target:RootId,args:&[RootId],map:bool)->Result<RootId,RootedError>{let result=self.vm.embedding_clone_collection_insert(target,args,map);self.completion(result)}

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
    /// Mark a host object serializable only when listed for transfer, as a MessagePort.
    pub fn require_transfer_rooted(&mut self, value: RootId) -> Result<(), RootedError> {
        self.vm.embedding_require_transfer(value).map_err(|error| self.error(error))
    }

    /// Serialize `value` for another agent: `transfers` (ArrayBuffers) move into the
    /// result, and each of `host_objects` becomes a reference the receiver supplies.
    pub fn serialize_rooted(
        &mut self,
        value: RootId,
        transfers: &[RootId],
        host_objects: &[RootId],
    ) -> Result<crate::SerializedValue, RootedError> {
        self.vm
            .embedding_serialize(value, transfers, host_objects)
            .map_err(|error| self.error(error))
    }

    /// Deserialize into this heap, substituting `host_objects` in the sender's order.
    pub fn deserialize_rooted(
        &mut self,
        serialized: crate::SerializedValue,
        host_objects: &[RootId],
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_deserialize(serialized, host_objects);
        self.completion(result)
    }

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

    /// Create a native host-private symbol. Explicit key operations work, while
    /// ordinary reflection and property copying never expose the key.
    pub fn private_symbol_rooted(&mut self, description: Option<&str>) -> RootId {
        let symbol = self.vm.embedding_private_symbol(description.map(str::to_owned));
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
    /// Compute the real byte offset needed to align an ArrayBuffer's backing
    /// address. Fixed buffers retain their physical address across snapshots
    /// and writes; resizable buffers retain it only until they resize.
    pub fn array_buffer_aligned_offset_rooted(&mut self, root: RootId, alignment: usize) -> Result<usize, RootedError> {
        self.vm.embedding_array_buffer_aligned_offset(root, alignment).map_err(|error| self.error(error))
    }

    pub fn uint8_array_rooted(&mut self, bytes: &[u8]) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_uint8_array(bytes);
        self.completion(result)
    }

    /// A Uint8Array that takes ownership of `bytes` as its backing store.
    pub fn uint8_array_from_rooted(&mut self, bytes: Vec<u8>) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_uint8_array_from(bytes);
        self.completion(result)
    }

    /// Retain a private weak reference without ECMAScript kept-object effects.
    pub fn host_weak_root(&mut self, target: RootId) -> Result<HostWeakRoot, RootedError> {
        self.vm
            .embedding_host_weak_root(target)
            .map(HostWeakRoot)
            .map_err(|error| self.error(error))
    }

    /// Produce the same non-coercing diagnostic description as Runtime.
    pub fn detail_string_rooted(&mut self, value: RootId) -> Result<String, RootedError> {
        self.vm.embedding_detail_string(value).map_err(|error| self.error(error))
    }

    /// Diagnostic own-property test shared with Runtime; no accessors or Proxy traps.
    pub fn has_own_property_rooted(&mut self, object: RootId, name: &str) -> Result<bool, RootedError> {
        self.vm.embedding_has_own_property(object, name).map_err(|error| self.error(error))
    }

    /// The VM's unforgeable Error brand, without guest property access.
    pub fn is_error_rooted(&mut self, value: RootId) -> Result<bool, RootedError> {
        self.vm.embedding_is_error(value).map_err(|error| self.error(error))
    }

    /// Probe a private weak reference without creating a temporary strong root
    /// or changing ECMAScript kept-object state, exactly as Runtime does.
    pub fn host_weak_root_is_live(&self, reference: HostWeakRoot) -> bool {
        self.vm.embedding_host_weak_value(reference.0).is_some()
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

    /// The number of collections so far. Host weak references can only have died when it
    /// changed, so a host prunes its weak tables when it does instead of on a timer.
    pub fn collection_epoch(&self) -> u64 {
        self.vm.embedding_collection_epoch()
    }

    /// Copy the raw bytes from a typed array or DataView without per-index guest property reads.
    pub fn view_bytes_rooted(&self, root: RootId) -> Option<Vec<u8>> {
        self.vm
            .root_value(root)
            .and_then(|value| self.vm.embedding_view_bytes(value))
    }

    /// Run `f` over the bytes of an ArrayBuffer, SharedArrayBuffer, typed array or
    /// DataView in place; `None` for any other value.
    pub fn with_buffer_bytes_rooted<T>(&self, root: RootId, f: impl FnOnce(&[u8]) -> T) -> Option<T> {
        self.vm.root_value(root).and_then(|value| self.vm.embedding_with_buffer_bytes(value, f))
    }

    /// Run `f` over those bytes for writing in place; `None` for any other value or a
    /// detached or immutable buffer.
    pub fn with_buffer_bytes_mut_rooted<T>(&mut self, root: RootId, f: impl FnOnce(&mut [u8]) -> T) -> Option<T> {
        let value = self.vm.root_value(root)?;
        self.vm.embedding_with_buffer_bytes_mut(value, f)
    }

    /// Run `f` over the bytes a typed array or DataView views, without copying them.
    pub fn with_view_bytes_rooted<T>(&self, root: RootId, f: impl FnOnce(&[u8]) -> T) -> Option<T> {
        self.vm.root_value(root).and_then(|value| self.vm.embedding_with_view_bytes(value, f))
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

    /// Create a JavaScript string from text decoded out of data (a file's or a buffer's
    /// bytes), which is not looked up among the atoms as `string_rooted`'s property keys are:
    /// hashing a long decoded text to find no atom cost as much as copying it.
    pub fn decoded_string_rooted(&mut self, text: &str) -> RootId {
        let root = self.vm.decoded_string_rooted(text);
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

    /// The ordinary object `{ [names[0]]: values[0], ... }`, made at its final shape
    /// (`Vm::create_embedding_record`): what an object literal of those names makes.
    pub fn record_rooted(&mut self, names: &'static [&'static str], values: &[RootId]) -> Result<RootId, RootedError> {
        let result = self.vm.create_embedding_record(names, values);
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

    /// Create an ordinary constructor with its immutable JavaScript creation name.
    pub fn host_constructor_named(
        &mut self,
        id: HostFunctionId,
        name: &str,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.create_named_host_constructor(id, name);
        self.completion(result)
    }

    /// Retain traced private data on a named ordinary constructor.
    pub fn host_constructor_named_with_data(
        &mut self,
        id: HostFunctionId,
        name: &str,
        data: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_value(data)
            .and_then(|value| self.vm.create_named_host_constructor_with_data(id, name, value));
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

    /// `object[name]` for a key given as Rust text; equivalent to `get_property_rooted` with
    /// a string key, without allocating the key.
    pub fn get_named_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.get_named_property_rooted(object, name);
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

    /// Perform the same strict keyed assignment as guest JavaScript.
    pub fn assign_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        value: RootId,
    ) -> Result<(), RootedError> {
        self.vm
            .assign_property_rooted(object, key, value)
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

    /// `define_data_property_rooted` with a string key the host spells, without allocating
    /// the key where the object is plain.
    pub fn define_named_data_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
        value: RootId,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<bool, RootedError> {
        let result = self.vm.define_named_data_property_rooted(
            object,
            name,
            value,
            writable,
            enumerable,
            configurable,
        );
        result.map_err(|error| self.error(error))
    }

    /// `set_property_rooted` with the receiver `object` and a string key the host spells,
    /// without allocating the key.
    pub fn set_named_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
        value: RootId,
    ) -> Result<bool, RootedError> {
        let result = self.vm.set_named_property_rooted(object, name, value);
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

    /// Runs the initializer of `name`'s lazy group on `holder` if it is still lazy; otherwise
    /// does nothing, and runs no guest code.
    /// The pending value a binding of `holder`'s member `key` holds while the member is still a
    /// lazy data property, or `None` when it is not. `listener` is then called with `index` and
    /// the member's value when its group materializes; until then script's writes and deletes of
    /// the member materialize the group first, and reading the pending value through a binding
    /// reads the member.
    pub fn pending_lazy_data_property_rooted(
        &mut self,
        holder: RootId,
        key: RootId,
        listener: RootId,
        index: u32,
    ) -> Result<Option<RootId>, RootedError> {
        let pending = self
            .vm
            .pending_lazy_data_property_rooted(holder, key, listener, index)
            .map_err(|error| self.error(error))?;
        Ok(pending.map(|value| self.scoped_value(value)))
    }

    pub fn materialize_lazy_data_property_rooted(
        &mut self,
        holder: RootId,
        name: &str,
    ) -> Result<(), RootedError> {
        self.vm
            .materialize_lazy_data_property_rooted(holder, name)
            .map_err(|error| self.error(error))
    }

    /// Whether `holder`'s own property `name` is still a lazy data property, observing nothing:
    /// its group has not materialized and script has not replaced it, so it holds the value
    /// its initializer will define.
    pub fn is_pending_lazy_data_property_rooted(
        &mut self,
        holder: RootId,
        name: &str,
    ) -> Result<bool, RootedError> {
        self.vm
            .is_pending_lazy_data_property_rooted(holder, name)
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

    /// Defines `members` (a name and whether it is enumerable) on `holder` as one group of
    /// lazy data properties, like `define_lazy_data_properties_rooted` with each member's own
    /// enumerability.
    pub fn define_lazy_data_members_rooted(
        &mut self,
        holder: RootId,
        members: &[(&str, bool)],
        initializer: RootId,
    ) -> Result<(), RootedError> {
        self.vm
            .define_lazy_group_rooted(holder, members, false, initializer)
            .map_err(|error| self.error(error))
    }

    /// Defines `members` (a name and whether it is enumerable) on `holder` as one group of
    /// lazy data properties, like `define_lazy_data_properties_rooted`, whose `initializer`
    /// also adds keys the members do not name: listing `holder`'s keys with its symbols, or
    /// fixing its integrity, materializes the group first.
    pub fn define_lazy_holder_keys_rooted(
        &mut self,
        holder: RootId,
        members: &[(&str, bool)],
        initializer: RootId,
    ) -> Result<(), RootedError> {
        self.vm
            .define_lazy_group_rooted(holder, members, true, initializer)
            .map_err(|error| self.error(error))
    }

    /// Defines `members` (a string or symbol key, whether it is enumerable while pending, and
    /// whether `initializer` defines it itself) on `holder` as one group of lazy properties.
    /// A member the initializer defines is removed before it runs, so it gets the attributes
    /// and position the initializer gives it; the others become writable data properties in
    /// place, as `define_lazy_data_properties_rooted` leaves them. While any member is
    /// redefined, script's writes and deletes of the members materialize the group first.
    pub fn define_lazy_members_rooted(
        &mut self,
        holder: RootId,
        members: &[(RootId, bool, bool)],
        initializer: RootId,
    ) -> Result<(), RootedError> {
        self.vm
            .define_lazy_members_rooted(holder, members, initializer)
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

    /// Take handled notifications from the VM's existing rejection authority.
    /// Returned event roots are scoped to this NativeContext frame. Retain a root
    /// explicitly to carry it beyond this callback; do not release scoped roots.
    pub fn take_promise_rejection_handled_events_rooted(&mut self) -> Vec<crate::PromiseRejectionEvent> {
        let events = self.vm.take_promise_rejection_handled_events();
        self.scope_rejection_event_roots(&events);
        events
    }
    /// Snapshot pending unhandled notifications only after handled dispatch. Roots
    /// have the same scoped ownership as other NativeContext rooted operations.
    pub fn take_promise_rejection_unhandled_events_rooted(&mut self) -> Vec<crate::PromiseRejectionEvent> {
        let events = self.vm.take_promise_rejection_unhandled_events();
        self.scope_rejection_event_roots(&events);
        events
    }
    fn scope_rejection_event_roots(&mut self, events: &[crate::PromiseRejectionEvent]) {
        for event in events {
            match event {
                crate::PromiseRejectionEvent::Handled { promise, .. } => self.roots.push(*promise),
                crate::PromiseRejectionEvent::Unhandled { promise, reason, .. } => {
                    self.roots.push(*promise);
                    self.roots.push(*reason);
                }
            }
        }
    }
    pub fn mark_promise_rejection_reported(&mut self, promise: RootId) -> Result<(), RootedError> {
        let promise = self.vm.embedding_value(promise).map_err(|error| self.error(error))?;
        self.vm.mark_promise_rejection_reported(promise).map_err(|error| self.error(error))
    }
    /// The embedder calls this once after completing the outer job's ticks,
    /// reactions and rejection policy. Never run automatically on C reentry.
    pub fn clear_kept_objects(&mut self) {
        self.vm.clear_kept_objects();
    }

    /// Whether the VM is already executing/draining ECMAScript jobs. A native
    /// embedder must defer its entire task checkpoint (including nextTick and
    /// rejection policy) in this case, rather than only skipping microtasks.
    /// This projects the same VM-wide guard used by run_microtask_checkpoint.
    pub fn is_microtask_checkpoint_active(&self) -> bool {
        self.vm.native_microtask_checkpoint_is_active()
    }

    /// Run currently queued ECMAScript jobs at a native MakeCallback boundary.
    /// This does not run host agent tasks (for example ready waitAsync tasks).
    /// Node owns nextTick-before-microtask ordering and callback depth. A native
    /// checkpoint requested from an already running job is deferred to its drain.
    pub fn run_microtask_checkpoint(&mut self) -> Result<(), RootedError> {
        let program = self.vm.embedding_program().map_err(|error| self.error(error))?;
        let result = self.vm.native_microtask_checkpoint(&program);
        result.map(drop).map_err(|error| self.error(error))
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

    /// Runs `step` and then releases the roots it created, so a long host call made of steps
    /// that store their results in the heap (a bootstrap's installers) holds only one step's
    /// handles at a time. On failure they stay, as the error's exception root must.
    pub fn scoped(
        &mut self,
        step: impl FnOnce(&mut Self) -> Result<(), RootedError>,
    ) -> Result<(), RootedError> {
        let mark = self.roots.len();
        let scoped = self.vm.scoped_root_height();
        step(self)?;
        self.vm.release_root_scope(scoped);
        for root in self.roots.drain(mark..) {
            self.vm.release_root(root);
        }
        Ok(())
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

/// The largest root list a finished native call returns for reuse; a call that scoped more
/// values frees its list rather than keep that capacity alive for every later call.
const RETAINED_NATIVE_ROOT_LIST_CAPACITY: usize = 256;

impl<H: Host> Drop for NativeContext<'_, H> {
    fn drop(&mut self) {
        self.vm.release_root_scope(self.scoped_height);
        for root in &self.roots {
            self.vm.release_root(*root);
        }
        let mut roots = std::mem::take(&mut self.roots);
        if roots.capacity() <= RETAINED_NATIVE_ROOT_LIST_CAPACITY {
            roots.clear();
            self.vm.native_root_lists.push(roots);
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
