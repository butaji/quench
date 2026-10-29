//! The `WebAssembly` namespace: the JavaScript interface to the shared Wasm machinery.
//!
//! Memories, tables and globals are their own `WebAssembly.Memory`, `.Table` and `.Global`
//! objects; modules and instances are `Cell::WasmObject`s. Values cross the boundary through
//! ToJSValue and ToWebAssemblyValue, so Wasm code only ever sees encoded Wasm values: a Wasm
//! function reaches JavaScript as an exported-function wrapper, and a JavaScript callable
//! reaches Wasm as a host function that converts on each call.

use super::promise::PromiseState;
use super::*;
use crate::heap::{WasmHostTarget, WasmObjectSlot};
use crate::{WasmType, WasmValue};
use wasmparser::ExternalKind;

/// Arguments of a Wasm call that a JavaScript caller passes without a heap allocation.
const WASM_INLINE_ARGUMENTS: usize = 8;

/// Whether a value of `ty` may encode into a fresh heap cell (`Vm::encode_wasm_value`): 64-bit
/// payloads outside the in-place range (`Vm::encode_bits64`) and vectors. 32-bit values encode
/// in place and references as the values they are.
fn encoding_allocates(ty: WasmType) -> bool {
    matches!(ty, WasmType::I64 | WasmType::F64 | WasmType::V128)
}

const DIRECT_EMSCRIPTEN_COPY_ENV: &str = "QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT";
const RAW_I32_EMSCRIPTEN_COPY_ENV: &str = "QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32";
const EARLY_IMPORT_EMSCRIPTEN_COPY_ENV: &str = "QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT";

static DIRECT_EMSCRIPTEN_COPY_SWITCH: std::sync::OnceLock<Option<bool>> =
    std::sync::OnceLock::new();
static RAW_I32_EMSCRIPTEN_COPY_SWITCH: std::sync::OnceLock<Option<bool>> =
    std::sync::OnceLock::new();
static EARLY_IMPORT_EMSCRIPTEN_COPY_SWITCH: std::sync::OnceLock<Option<bool>> =
    std::sync::OnceLock::new();

#[cfg(test)]
thread_local! {
    static DIRECT_EMSCRIPTEN_COPY_TEST_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
    static RAW_I32_EMSCRIPTEN_COPY_TEST_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
    static EARLY_IMPORT_EMSCRIPTEN_COPY_TEST_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// Whether the guarded direct-copy path is enabled in this process. Separate
/// processes can use this switch for an on/off A/B without rebuilding.
pub(super) fn emscripten_memcpy_direct_copy_enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = DIRECT_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return enabled;
    }

    DIRECT_EMSCRIPTEN_COPY_SWITCH
        .get_or_init(
            || match std::env::var(DIRECT_EMSCRIPTEN_COPY_ENV).as_deref() {
                Ok("0") => Some(false),
                Ok("1") => Some(true),
                _ => None,
            },
        )
        .unwrap_or(true)
}

pub(super) fn emscripten_memcpy_direct_copy_switch() -> &'static str {
    #[cfg(test)]
    if let Some(enabled) = DIRECT_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return if enabled { "1" } else { "0" };
    }

    match DIRECT_EMSCRIPTEN_COPY_SWITCH.get_or_init(|| {
        match std::env::var(DIRECT_EMSCRIPTEN_COPY_ENV).as_deref() {
            Ok("0") => Some(false),
            Ok("1") => Some(true),
            _ => None,
        }
    }) {
        Some(false) => "0",
        Some(true) => "1",
        None => "default",
    }
}

/// The raw-argument import path is opt-in so the late-bound path remains the
/// control arm unless an experiment explicitly selects it.
pub(super) fn emscripten_memcpy_raw_i32_enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = RAW_I32_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return enabled;
    }

    RAW_I32_EMSCRIPTEN_COPY_SWITCH
        .get_or_init(
            || match std::env::var(RAW_I32_EMSCRIPTEN_COPY_ENV).as_deref() {
                Ok("0") => Some(false),
                Ok("1") => Some(true),
                _ => None,
            },
        )
        .unwrap_or(false)
}

pub(super) fn emscripten_memcpy_raw_i32_switch() -> &'static str {
    #[cfg(test)]
    if let Some(enabled) = RAW_I32_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return if enabled { "1" } else { "0" };
    }

    match RAW_I32_EMSCRIPTEN_COPY_SWITCH.get_or_init(|| {
        match std::env::var(RAW_I32_EMSCRIPTEN_COPY_ENV).as_deref() {
            Ok("0") => Some(false),
            Ok("1") => Some(true),
            _ => None,
        }
    }) {
        Some(false) => "0",
        Some(true) => "1",
        None => "default",
    }
}

/// Whether compiled import callouts may try the raw memcpy guard before the
/// general Wasm import dispatcher. The exact-import and current-view checks
/// remain mandatory; this switch only selects where they run.
pub(super) fn emscripten_memcpy_early_import_enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = EARLY_IMPORT_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return enabled;
    }

    EARLY_IMPORT_EMSCRIPTEN_COPY_SWITCH
        .get_or_init(
            || match std::env::var(EARLY_IMPORT_EMSCRIPTEN_COPY_ENV).as_deref() {
                Ok("0") => Some(false),
                Ok("1") => Some(true),
                _ => None,
            },
        )
        .unwrap_or(false)
}

pub(super) fn emscripten_memcpy_early_import_switch() -> &'static str {
    #[cfg(test)]
    if let Some(enabled) = EARLY_IMPORT_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return if enabled { "1" } else { "0" };
    }

    match EARLY_IMPORT_EMSCRIPTEN_COPY_SWITCH.get_or_init(|| {
        match std::env::var(EARLY_IMPORT_EMSCRIPTEN_COPY_ENV).as_deref() {
            Ok("0") => Some(false),
            Ok("1") => Some(true),
            _ => None,
        }
    }) {
        Some(false) => "0",
        Some(true) => "1",
        None => "default",
    }
}

pub(super) fn copy_within_relative_index(index: i64, length: usize) -> usize {
    if index < 0 {
        let magnitude = usize::try_from(index.unsigned_abs()).unwrap_or(usize::MAX);
        length.saturating_sub(magnitude)
    } else {
        usize::try_from(index).unwrap_or(usize::MAX).min(length)
    }
}

fn note_emscripten_memcpy_fast_path(counter: *mut u64) {
    if !counter.is_null() {
        // SAFETY: diagnostics allocate this counter for the duration of the
        // compiled invocation; normal execution passes a null pointer.
        unsafe {
            *counter = (*counter).saturating_add(1);
        }
    }
}

fn note_emscripten_memcpy_raw_i32_hit(counter: *mut u64) {
    if !counter.is_null() {
        // SAFETY: the diagnostic counter lives for the compiled invocation.
        unsafe {
            *counter = (*counter).saturating_add(1);
        }
    }
}

#[cfg(test)]
pub(super) fn with_emscripten_memcpy_direct_copy_for_test<R>(
    enabled: bool,
    run: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<bool>);

    impl Drop for Restore {
        fn drop(&mut self) {
            DIRECT_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(|value| value.set(self.0));
        }
    }

    let previous = DIRECT_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(|value| value.replace(Some(enabled)));
    let _restore = Restore(previous);
    run()
}

#[cfg(test)]
pub(super) fn with_emscripten_memcpy_raw_i32_for_test<R>(
    enabled: bool,
    run: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<bool>);

    impl Drop for Restore {
        fn drop(&mut self) {
            RAW_I32_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(|value| value.set(self.0));
        }
    }

    let previous = RAW_I32_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(|value| value.replace(Some(enabled)));
    let _restore = Restore(previous);
    run()
}

#[cfg(test)]
pub(super) fn with_emscripten_memcpy_early_import_for_test<R>(
    enabled: bool,
    run: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<bool>);

    impl Drop for Restore {
        fn drop(&mut self) {
            EARLY_IMPORT_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(|value| value.set(self.0));
        }
    }

    let previous =
        EARLY_IMPORT_EMSCRIPTEN_COPY_TEST_OVERRIDE.with(|value| value.replace(Some(enabled)));
    let _restore = Restore(previous);
    run()
}

/// The main realm's interface prototypes, which every new Wasm object of the matching kind
/// receives. Null until the namespace is installed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WasmJsIntrinsics {
    pub(crate) module: Value,
    pub(crate) instance: Value,
    pub(crate) memory: Value,
    pub(crate) table: Value,
    pub(crate) global: Value,
    /// `WebAssembly.JSTag`: the tag whose one `externref` parameter carries a JavaScript
    /// exception through Wasm.
    pub(crate) js_tag: Value,
}

impl Default for WasmJsIntrinsics {
    fn default() -> Self {
        Self {
            module: Value::NULL,
            instance: Value::NULL,
            memory: Value::NULL,
            table: Value::NULL,
            global: Value::NULL,
            js_tag: Value::NULL,
        }
    }
}

impl WasmJsIntrinsics {
    pub(crate) fn roots(&self) -> [Value; 6] {
        [
            self.module,
            self.instance,
            self.memory,
            self.table,
            self.global,
            self.js_tag,
        ]
    }
}

/// The interface constructors: name, constructor, prototype methods and accessors.
struct Interface {
    name: &'static str,
    constructor: Native,
    statics: &'static [(&'static str, Native)],
    methods: &'static [(&'static str, Native)],
    getters: &'static [(&'static str, Native, Option<Native>)],
}

const INTERFACES: &[Interface] = &[
    Interface {
        name: "Module",
        constructor: Native::WasmModule,
        statics: &[
            ("exports", Native::WasmModuleExports),
            ("imports", Native::WasmModuleImports),
            ("customSections", Native::WasmModuleCustomSections),
        ],
        methods: &[],
        getters: &[],
    },
    Interface {
        name: "Instance",
        constructor: Native::WasmInstance,
        statics: &[],
        methods: &[],
        getters: &[("exports", Native::WasmInstanceExportsGetter, None)],
    },
    Interface {
        name: "Memory",
        constructor: Native::WasmMemory,
        statics: &[],
        methods: &[("grow", Native::WasmMemoryGrow)],
        getters: &[("buffer", Native::WasmMemoryBufferGetter, None)],
    },
    Interface {
        name: "Table",
        constructor: Native::WasmTable,
        statics: &[],
        methods: &[
            ("grow", Native::WasmTableGrow),
            ("get", Native::WasmTableGet),
            ("set", Native::WasmTableSet),
        ],
        getters: &[("length", Native::WasmTableLengthGetter, None)],
    },
    Interface {
        name: "Global",
        constructor: Native::WasmGlobal,
        statics: &[],
        methods: &[("valueOf", Native::WasmGlobalValueOf)],
        getters: &[(
            "value",
            Native::WasmGlobalValueGetter,
            Some(Native::WasmGlobalValueSetter),
        )],
    },
];

const NAMESPACE_FUNCTIONS: &[(&str, Native)] = &[
    ("compile", Native::WebAssemblyCompile),
    ("validate", Native::WebAssemblyValidate),
    ("instantiate", Native::WebAssemblyInstantiate),
];

pub(super) const WASM_ERRORS: &[(&str, Native)] = &[
    ("CompileError", Native::WasmCompileError),
    ("LinkError", Native::WasmLinkError),
    ("RuntimeError", Native::WasmRuntimeError),
];

/// The largest page count or element count a WebIDL `unsigned long` descriptor member holds.
const DESCRIPTOR_LIMIT: f64 = u32::MAX as f64;

pub(super) fn is_wasm_js_native(native: Native) -> bool {
    matches!(
        native,
        Native::WebAssemblyValidate
            | Native::WebAssemblyCompile
            | Native::WebAssemblyInstantiate
            | Native::WasmModule
            | Native::WasmModuleExports
            | Native::WasmModuleImports
            | Native::WasmModuleCustomSections
            | Native::WasmInstance
            | Native::WasmInstanceExportsGetter
            | Native::WasmMemory
            | Native::WasmMemoryGrow
            | Native::WasmMemoryBufferGetter
            | Native::WasmTable
            | Native::WasmTableLengthGetter
            | Native::WasmTableGrow
            | Native::WasmTableGet
            | Native::WasmTableSet
            | Native::WasmGlobal
            | Native::WasmGlobalValueGetter
            | Native::WasmGlobalValueSetter
            | Native::WasmGlobalValueOf
            | Native::WasmExportedFunction
    )
}

pub(super) fn is_wasm_js_constructor(native: Native) -> bool {
    matches!(
        native,
        Native::WasmModule
            | Native::WasmInstance
            | Native::WasmMemory
            | Native::WasmTable
            | Native::WasmGlobal
    )
}

pub(super) fn wasm_js_native_length(native: Native) -> Option<f64> {
    Some(match native {
        Native::WebAssemblyValidate
        | Native::WebAssemblyCompile
        | Native::WebAssemblyInstantiate
        | Native::WasmModule
        | Native::WasmModuleExports
        | Native::WasmModuleImports
        | Native::WasmInstance
        | Native::WasmMemory
        | Native::WasmMemoryGrow
        | Native::WasmTable
        | Native::WasmTableGrow
        | Native::WasmTableGet
        | Native::WasmTableSet
        | Native::WasmGlobal
        | Native::WasmGlobalValueSetter
        | Native::WasmCompileError
        | Native::WasmLinkError
        | Native::WasmRuntimeError => 1.0,
        Native::WasmInstanceExportsGetter
        | Native::WasmMemoryBufferGetter
        | Native::WasmTableLengthGetter
        | Native::WasmGlobalValueGetter
        | Native::WasmGlobalValueOf => 0.0,
        Native::WasmModuleCustomSections => 2.0,
        _ => return None,
    })
}

fn kind_name(kind: ExternalKind) -> &'static str {
    match kind {
        ExternalKind::Func | ExternalKind::FuncExact => "function",
        ExternalKind::Table => "table",
        ExternalKind::Memory => "memory",
        ExternalKind::Global => "global",
        ExternalKind::Tag => "tag",
    }
}

/// The reference types a `WebAssembly.Table` descriptor names.
fn descriptor_reference_type(name: &str) -> Option<wasmparser::RefType> {
    Some(match name {
        "anyfunc" => wasmparser::RefType::FUNCREF,
        "externref" => wasmparser::RefType::EXTERNREF,
        "anyref" => wasmparser::RefType::ANYREF,
        "eqref" => wasmparser::RefType::EQREF,
        "i31ref" => wasmparser::RefType::I31REF,
        "structref" => wasmparser::RefType::STRUCTREF,
        "arrayref" => wasmparser::RefType::ARRAYREF,
        _ => return None,
    })
}

/// The value types a `WebAssembly.Global` descriptor names.
fn descriptor_value_type(name: &str) -> Option<WasmType> {
    Some(match name {
        "i32" => WasmType::I32,
        "i64" => WasmType::I64,
        "f32" => WasmType::F32,
        "f64" => WasmType::F64,
        _ => return descriptor_reference_type(name).map(reference_value_type),
    })
}

/// The value type of a table's elements, as the JavaScript conversions see it.
fn reference_value_type(element_type: wasmparser::RefType) -> WasmType {
    reference_value_type_in(element_type, &crate::WasmTypes::default())
}

/// `reference_value_type` for a type `declarations` may name. A type the conversions have
/// no form for (a shared one) converts as `externref` did before GC types reached them.
fn reference_value_type_in(element_type: wasmparser::RefType, declarations: &crate::WasmTypes) -> WasmType {
    declarations
        .callable_value_type(wasmparser::ValType::Ref(element_type))
        .unwrap_or(WasmType::Reference {
            kind: crate::WasmReferenceKind::External,
            nullable: element_type.is_nullable(),
        })
}

impl<H: Host> Vm<H> {
    pub(super) fn install_webassembly(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let namespace = self.object();
        self.install_builtin_to_string_tag(namespace, "WebAssembly")?;
        for &(name, native) in NAMESPACE_FUNCTIONS {
            self.set_builtin_named(program, namespace, name, native)?;
            self.set_enumerable(namespace, name);
        }
        for interface in INTERFACES {
            let constructor = self.native_value(interface.constructor);
            self.set_builtin_function_name(constructor, interface.name)?;
            let prototype = self.object();
            self.set_named_constant(program, constructor, "prototype", prototype)?;
            self.set_builtin_value_named(prototype, "constructor", constructor)?;
            self.install_builtin_to_string_tag(
                prototype,
                &format!("WebAssembly.{}", interface.name),
            )?;
            for &(name, native) in interface.statics {
                self.set_builtin_named(program, constructor, name, native)?;
                self.set_enumerable(constructor, name);
            }
            for &(name, native) in interface.methods {
                self.set_builtin_named(program, prototype, name, native)?;
                self.set_enumerable(prototype, name);
            }
            for &(name, getter, setter) in interface.getters {
                let getter = self.native_value(getter);
                self.set_builtin_function_name(getter, &format!("get {name}"))?;
                let setter = setter.map(|setter| self.native_value(setter));
                if let Some(setter) = setter {
                    self.set_builtin_function_name(setter, &format!("set {name}"))?;
                }
                let atom = self.intern_atom(name);
                self.set_property_with_attributes(
                    prototype,
                    atom,
                    Value::UNDEFINED,
                    PropertyAttributes {
                        writable: false,
                        enumerable: true,
                        configurable: true,
                        accessor: true,
                        getter: Some(getter),
                        setter,
                    },
                )?;
            }
            let slot = match interface.constructor {
                Native::WasmModule => &mut self.wasm_js.module,
                Native::WasmInstance => &mut self.wasm_js.instance,
                Native::WasmMemory => &mut self.wasm_js.memory,
                Native::WasmTable => &mut self.wasm_js.table,
                _ => &mut self.wasm_js.global,
            };
            *slot = prototype;
            self.set_builtin_value_named(namespace, interface.name, constructor)?;
        }
        self.install_wasm_errors(program, namespace)?;
        let js_tag = self.heap.alloc(js_tag_cell());
        self.wasm_js.js_tag = js_tag;
        let atom = self.intern_atom("JSTag");
        self.set_property_with_attributes(
            namespace,
            atom,
            js_tag,
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        )?;
        self.global(program, "WebAssembly", namespace)
    }

    /// The namespace's error classes: NativeError-shaped constructors inheriting from
    /// `Error`.
    fn install_wasm_errors(
        &mut self,
        program: &ResidualProgram,
        namespace: Value,
    ) -> Result<(), JsError> {
        let error_prototype =
            self.realm.intrinsics.builtin_prototypes[&(self.realm.globals, Native::Error)];
        for &(name, native) in WASM_ERRORS {
            let prototype = self
                .heap
                .alloc(Cell::Object(Self::empty_object(error_prototype)));
            self.install_error_class(program, native, name, prototype)?;
            let native_function = self.native_value(native);
            self.set_builtin_value_named(namespace, name, native_function)?;
        }
        Ok(())
    }

    fn set_enumerable(&mut self, object: Value, name: &str) {
        let atom = self.intern_atom(name);
        self.set_property_attributes(
            object,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable: true,
                enumerable: true,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
    }

    /// A thrown `WebAssembly` error of class `native`.
    pub(super) fn wasm_js_error(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        text: String,
    ) -> JsError {
        let message = self.heap.alloc_string(JsString::from_str(&text));
        let object = self
            .construct_error_native(program, native, &[message])
            .unwrap_or(Value::UNDEFINED);
        JsError::thrown(object, text)
    }

    pub(super) fn call_wasm_js_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let arg = |index: usize| args.get(index).copied().unwrap_or(Value::UNDEFINED);
        match native {
            Native::WebAssemblyValidate => {
                let bytes = self.wasm_source_bytes(p, arg(0))?;
                let mode = self.execution_mode;
                Ok(if self.host.compile_wasm(&bytes, mode).is_ok() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::WebAssemblyCompile => {
                let module = self.wasm_compile_module(p, arg(0));
                self.settled_promise(p, module)
            }
            Native::WebAssemblyInstantiate => {
                let result = self.wasm_instantiate_source(p, arg(0), arg(1));
                self.settled_promise(p, result)
            }
            Native::WasmModuleExports => {
                let module = self.wasm_module_of(p, arg(0))?;
                let descriptors = module
                    .exports()
                    .iter()
                    .map(|export| (export.name.clone(), export.kind))
                    .collect::<Vec<_>>();
                let mut values = Vec::with_capacity(descriptors.len());
                for (name, kind) in descriptors {
                    values.push(
                        self.wasm_descriptor(p, &[("name", &name), ("kind", kind_name(kind))])?,
                    );
                }
                Ok(self.new_array(values))
            }
            Native::WasmModuleCustomSections => {
                let module = self.wasm_module_of(p, arg(0))?;
                // `sectionName` is a required DOMString.
                if args.len() < 2 {
                    return Err(self.type_error(
                        p,
                        "WebAssembly.Module.customSections(): Argument 1 must be a string".into(),
                    ));
                }
                let name = self.to_string(p, arg(1))?;
                let name = String::from(name);
                let sections = module
                    .custom_sections_named(&name)
                    .map(<[u8]>::to_vec)
                    .collect::<Vec<_>>();
                let mut values = Vec::with_capacity(sections.len());
                for bytes in sections {
                    let length = bytes.len();
                    values.push(self.new_array_buffer_cell(bytes.into(), false, length, false, false));
                }
                Ok(self.new_array(values))
            }
            Native::WasmModuleImports => {
                let module = self.wasm_module_of(p, arg(0))?;
                let descriptors = module
                    .import_kinds()
                    .into_iter()
                    .map(|(name, kind)| (name.module.clone(), name.name.clone(), kind))
                    .collect::<Vec<_>>();
                let mut values = Vec::with_capacity(descriptors.len());
                for (module, name, kind) in descriptors {
                    values.push(self.wasm_descriptor(
                        p,
                        &[
                            ("module", &module),
                            ("name", &name),
                            ("kind", kind_name(kind)),
                        ],
                    )?);
                }
                Ok(self.new_array(values))
            }
            Native::WasmInstanceExportsGetter => match self.heap.get(this) {
                Some(Cell::WasmObject {
                    slot: WasmObjectSlot::Instance { exports },
                    ..
                }) => Ok(*exports),
                _ => Err(self.type_error(p, "receiver is not a WebAssembly.Instance".into())),
            },
            Native::WasmMemoryGrow => {
                self.wasm_memory_receiver(p, this)?;
                let delta = self.wasm_descriptor_u32(p, arg(0), "delta")?;
                match self.wasm_memory_grow(this, u64::from(delta))? {
                    Some(previous) => Ok(Value::number(previous as f64)),
                    None => Err(self.range_error(
                        p,
                        "WebAssembly.Memory.grow(): maximum memory size exceeded".into(),
                    )),
                }
            }
            Native::WasmMemoryBufferGetter => {
                self.wasm_memory_receiver(p, this)?;
                self.wasm_memory_buffer(this).ok_or_else(|| {
                    self.type_error(
                        p,
                        "a shared WebAssembly.Memory buffer is not supported".into(),
                    )
                })
            }
            Native::WasmTableLengthGetter => {
                let length = self.wasm_table_receiver(p, this)?;
                Ok(Value::number(length as f64))
            }
            Native::WasmTableGet => {
                let length = self.wasm_table_receiver(p, this)?;
                let index = self.wasm_js_table_index(p, arg(0), length)?;
                let value = self.wasm_table_get(this, index as u64)?;
                let ty = self.wasm_table_element_type(this);
                let declarations = self.wasm_table_declarations(this);
                self.wasm_to_js(p, value, ty, declarations.as_ref())
            }
            Native::WasmTableSet => {
                let length = self.wasm_table_receiver(p, this)?;
                let index = self.wasm_js_table_index(p, arg(0), length)?;
                let ty = self.wasm_table_element_type(this);
                let declarations = self.wasm_table_declarations(this);
                let value = match args.get(1) {
                    Some(value) => self.js_to_wasm(p, *value, ty, declarations.as_ref())?,
                    None => self.wasm_table_default(ty),
                };
                if let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(this) {
                    elements[index] = value;
                }
                self.invalidate_wasm_reference_caches();
                Ok(Value::UNDEFINED)
            }
            Native::WasmTableGrow => {
                let length = self.wasm_table_receiver(p, this)?;
                let delta = self.wasm_descriptor_u32(p, arg(0), "delta")?;
                let ty = self.wasm_table_element_type(this);
                let declarations = self.wasm_table_declarations(this);
                let value = match args.get(1) {
                    Some(value) => self.js_to_wasm(p, *value, ty, declarations.as_ref())?,
                    None => self.wasm_table_default(ty),
                };
                let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(this) else {
                    unreachable!("checked table receiver");
                };
                let grown = usize::try_from(delta)
                    .ok()
                    .and_then(|delta| length.checked_add(delta))
                    .filter(|grown| {
                        elements
                            .maximum
                            .is_none_or(|maximum| *grown as u64 <= maximum)
                            && *grown as u64 <= DESCRIPTOR_LIMIT as u64
                    });
                let Some(grown) = grown else {
                    return Err(self
                        .range_error(p, "WebAssembly.Table.grow(): failed to grow table".into()));
                };
                elements.resize(grown, value);
                self.invalidate_wasm_reference_caches();
                Ok(Value::number(length as f64))
            }
            Native::WasmGlobalValueGetter | Native::WasmGlobalValueOf => {
                let (value, ty, _) = self.wasm_global_receiver(p, this)?;
                let declarations = self.wasm_global_declarations(this);
                self.wasm_to_js(p, value, ty, declarations.as_ref())
            }
            Native::WasmGlobalValueSetter => {
                let (_, ty, mutable) = self.wasm_global_receiver(p, this)?;
                if !mutable {
                    return Err(self.type_error(
                        p,
                        "cannot set the value of an immutable WebAssembly.Global".into(),
                    ));
                }
                let declarations = self.wasm_global_declarations(this);
                let value = self.js_to_wasm(p, arg(0), ty, declarations.as_ref())?;
                if let Some(Cell::WasmGlobal { value: slot, .. }) = self.heap.get_mut(this) {
                    *slot = value;
                }
                Ok(Value::UNDEFINED)
            }
            Native::WasmExportedFunction => self.call_wasm_exported_function(p, args),
            native if is_wasm_js_constructor(native) => {
                let name = INTERFACES
                    .iter()
                    .find(|interface| interface.constructor == native)
                    .map_or("", |interface| interface.name);
                Err(self.type_error(
                    p,
                    format!("WebAssembly.{name}(): WebAssembly.{name} must be invoked with 'new'"),
                ))
            }
            _ => unreachable!("not a WebAssembly native"),
        }
    }

    pub(super) fn construct_wasm_js_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let arg = |index: usize| args.get(index).copied().unwrap_or(Value::UNDEFINED);
        match native {
            Native::WasmModule => self.wasm_compile_module(p, arg(0)),
            Native::WasmInstance => {
                let module = self.wasm_module_of(p, arg(0))?;
                self.wasm_instantiate(p, &module, arg(1))
            }
            Native::WasmMemory => {
                let descriptor = self.wasm_descriptor_object(p, arg(0))?;
                let initial = self.wasm_descriptor_member(p, descriptor, "initial")?;
                let initial = self.wasm_descriptor_u32(p, initial, "initial")?;
                let maximum = self.wasm_descriptor_member(p, descriptor, "maximum")?;
                let maximum = (!maximum.is_undefined())
                    .then(|| self.wasm_descriptor_u32(p, maximum, "maximum"))
                    .transpose()?;
                let shared = self.wasm_descriptor_member(p, descriptor, "shared")?;
                let shared = self.truthy(shared);
                let ty = wasmparser::MemoryType {
                    memory64: false,
                    shared,
                    initial: u64::from(initial),
                    maximum: maximum.map(u64::from),
                    page_size_log2: None,
                };
                if maximum.is_some_and(|maximum| maximum < initial)
                    || !crate::wasm::memory::supported_memory(&ty)
                    || u64::from(initial) > crate::wasm::memory::memory_page_limit(&ty)
                    || (shared && maximum.is_none())
                {
                    return Err(self
                        .range_error(p, "WebAssembly.Memory(): invalid memory descriptor".into()));
                }
                self.alloc_wasm_memory(ty).map_err(|_| {
                    self.range_error(p, "WebAssembly.Memory(): could not allocate memory".into())
                })
            }
            Native::WasmTable => {
                let descriptor = self.wasm_descriptor_object(p, arg(0))?;
                let element = self.wasm_descriptor_member(p, descriptor, "element")?;
                let element = self.to_string(p, element)?;
                let Some(element_type) = descriptor_reference_type(&element) else {
                    return Err(
                        self.type_error(p, "WebAssembly.Table(): invalid element type".into())
                    );
                };
                let initial = self.wasm_descriptor_member(p, descriptor, "initial")?;
                let initial = self.wasm_descriptor_u32(p, initial, "initial")?;
                let maximum = self.wasm_descriptor_member(p, descriptor, "maximum")?;
                let maximum = (!maximum.is_undefined())
                    .then(|| self.wasm_descriptor_u32(p, maximum, "maximum"))
                    .transpose()?;
                if maximum.is_some_and(|maximum| maximum < initial) {
                    return Err(
                        self.range_error(p, "WebAssembly.Table(): maximum is below initial".into())
                    );
                }
                let ty = reference_value_type(element_type);
                let initial_value = match args.get(1) {
                    Some(value) if !value.is_undefined() => self.js_to_wasm(p, *value, ty, None)?,
                    _ => self.wasm_table_default(ty),
                };
                self.alloc_wasm_table(
                    wasmparser::TableType {
                        element_type,
                        table64: false,
                        initial: u64::from(initial),
                        maximum: maximum.map(u64::from),
                        shared: false,
                    },
                    crate::WasmTypes::default(),
                    initial_value,
                )
            }
            Native::WasmGlobal => {
                let descriptor = self.wasm_descriptor_object(p, arg(0))?;
                let mutable = self.wasm_descriptor_member(p, descriptor, "mutable")?;
                let mutable = self.truthy(mutable);
                let ty = self.wasm_descriptor_member(p, descriptor, "value")?;
                let ty = self.to_string(p, ty)?;
                let Some(ty) = descriptor_value_type(&ty) else {
                    return Err(
                        self.type_error(p, "WebAssembly.Global(): invalid value type".into())
                    );
                };
                let value = match args.get(1) {
                    Some(value) if !value.is_undefined() => self.js_to_wasm(p, *value, ty, None)?,
                    _ => self.wasm_global_default(ty),
                };
                Ok(self.alloc_wasm_global(value, ty, crate::WasmTypes::default(), mutable))
            }
            _ => unreachable!("not a WebAssembly constructor"),
        }
    }

    /// A promise settled with `result`: fulfilled with its value or rejected with the
    /// thrown value of its error.
    fn settled_promise(
        &mut self,
        p: &ResidualProgram,
        result: Result<Value, JsError>,
    ) -> Result<Value, JsError> {
        let payload = match &result {
            Ok(value) => *value,
            Err(error) => error.thrown_value().unwrap_or(Value::UNDEFINED),
        };
        let promise = self.with_call_roots([payload], |vm| vm.promise_object())?;
        match result {
            Ok(value) => self.promise_resolve_value(p, promise, value)?,
            Err(error) => {
                let reason = self.thrown_value_for(p, error);
                self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
            }
        }
        Ok(promise)
    }

    fn wasm_source_bytes(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Vec<u8>, JsError> {
        match self.heap.get(source) {
            Some(Cell::ArrayBuffer {
                bytes, detached, ..
            }) => Ok(if *detached {
                Vec::new()
            } else {
                bytes.to_vec()
            }),
            _ => self.embedding_view_bytes(source).ok_or_else(|| {
                self.type_error(p, "WebAssembly source must be a BufferSource".into())
            }),
        }
    }

    fn wasm_compile_module(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Value, JsError> {
        let bytes = self.wasm_source_bytes(p, source)?;
        let mode = self.execution_mode;
        let module = self
            .host
            .compile_wasm(&bytes, mode)
            .map_err(|message| self.wasm_js_error(p, Native::WasmCompileError, message))?;
        Ok(self.heap.alloc(Cell::WasmObject {
            object: Box::new(Self::empty_object(self.wasm_js.module)),
            slot: WasmObjectSlot::Module(Box::new(module)),
        }))
    }

    /// `WebAssembly.instantiate`: a module source instantiates to its instance alone;
    /// bytes compile and instantiate to a `{ module, instance }` result.
    fn wasm_instantiate_source(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        imports: Value,
    ) -> Result<Value, JsError> {
        if let Some(Cell::WasmObject {
            slot: WasmObjectSlot::Module(module),
            ..
        }) = self.heap.get(source)
        {
            let module = (**module).clone();
            return self.wasm_instantiate(p, &module, imports);
        }
        let module_object = self.wasm_compile_module(p, source)?;
        let module_root = self.heap.root(module_object);
        let module = self.wasm_module_of(p, module_object)?;
        let instance = self.wasm_instantiate(p, &module, imports);
        let module_object = self.heap.root_value(module_root).expect("rooted module");
        self.heap.release_root(module_root);
        let instance = instance?;
        let result = self.object();
        self.set_named(p, result, "module", module_object)?;
        self.set_named(p, result, "instance", instance)?;
        Ok(result)
    }

    fn wasm_module_of(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<crate::WasmModule, JsError> {
        match self.heap.get(value) {
            Some(Cell::WasmObject {
                slot: WasmObjectSlot::Module(module),
                ..
            }) => Ok((**module).clone()),
            _ => Err(self.type_error(p, "argument must be a WebAssembly.Module".into())),
        }
    }

    /// Resolves each import from `imports`, instantiates `module` and returns its
    /// `WebAssembly.Instance`.
    fn wasm_instantiate(
        &mut self,
        p: &ResidualProgram,
        module: &crate::WasmModule,
        imports: Value,
    ) -> Result<Value, JsError> {
        let import_kinds = module
            .import_kinds()
            .into_iter()
            .map(|(name, kind)| (name.clone(), kind))
            .collect::<Vec<_>>();
        if !imports.is_undefined() && !self.is_object_like(imports) {
            return Err(self.type_error(p, "WebAssembly imports argument must be an object".into()));
        }
        if !import_kinds.is_empty() && imports.is_undefined() {
            return Err(self.type_error(p, "WebAssembly module requires an imports object".into()));
        }
        let mut roots = Vec::with_capacity(import_kinds.len());
        let resolved = (|| {
            let mut function_index = 0;
            for (name, kind) in &import_kinds {
                let module_atom = self.intern_atom(&name.module);
                let namespace = self.get_property(p, imports, module_atom)?;
                if !self.is_object_like(namespace) {
                    return Err(self.type_error(
                        p,
                        format!(
                            "WebAssembly import module '{}' must be an object",
                            name.module
                        ),
                    ));
                }
                let field_atom = self.intern_atom(&name.name);
                let value = self.get_property(p, namespace, field_atom)?;
                let value = match kind {
                    ExternalKind::Func | ExternalKind::FuncExact => {
                        let index = function_index;
                        function_index += 1;
                        self.wasm_import_function(p, module, index, value, name)?
                    }
                    ExternalKind::Global => self.wasm_import_global(p, module, name, value)?,
                    ExternalKind::Memory => self.wasm_import_object(p, value, name, |cell| {
                        matches!(cell, Cell::WasmMemory { .. })
                    })?,
                    ExternalKind::Table => self.wasm_import_object(p, value, name, |cell| {
                        matches!(cell, Cell::WasmTable { .. })
                    })?,
                    ExternalKind::Tag => self.wasm_import_object(p, value, name, |cell| {
                        matches!(cell, Cell::WasmTag { .. })
                    })?,
                };
                roots.push(self.root(value));
            }
            self.instantiate_wasm_module_with_imports(module, &roots)
        })();
        for root in roots {
            self.release_root(root);
        }
        let instance = resolved?;
        let exports = self.wasm_exports_object(p, &instance);
        self.release_root(instance.environment);
        let exports = exports?;
        Ok(self.heap.alloc(Cell::WasmObject {
            object: Box::new(Self::empty_object(self.wasm_js.instance)),
            slot: WasmObjectSlot::Instance { exports },
        }))
    }

    fn wasm_import_function(
        &mut self,
        p: &ResidualProgram,
        module: &crate::WasmModule,
        index: usize,
        value: Value,
        name: &crate::WasmImportName,
    ) -> Result<Value, JsError> {
        if !self.is_function(value) {
            return Err(self.wasm_js_error(
                p,
                Native::WasmLinkError,
                format!(
                    "WebAssembly import '{}.{}' must be callable",
                    name.module, name.name
                ),
            ));
        }
        if let Some(closure) = self.wasm_exported_closure(value) {
            return Ok(closure);
        }
        let signature = module
            .signatures
            .get(index)
            .cloned()
            .ok_or_else(|| JsError::validation("invalid Wasm import signature".into()))?;
        let target = self
            .emscripten_memcpy_view_capture(value, &signature)
            .map_or(
                WasmHostTarget::Js(value),
                |(view_environment, view_slot)| {
                    WasmHostTarget::JsEmscriptenMemcpy(Box::new(
                        crate::heap::WasmHostMemcpyTarget {
                            function: value,
                            view_environment,
                            view_slot,
                        },
                    ))
                },
            );
        self.create_owned_wasm_host_function(
            &name.name,
            target,
            signature,
            Some(module.signatures.clone()),
        )
    }

    /// Recognizes only the canonical Emscripten helper body and its real mutable
    /// lexical `HEAPU8` binding. The capture location is retained; the view itself
    /// is deliberately re-read on every call because Emscripten replaces it after
    /// `WebAssembly.Memory.grow()`.
    pub(super) fn emscripten_memcpy_view_capture(
        &self,
        function: Value,
        signature: &crate::WasmSignature,
    ) -> Option<(Value, usize)> {
        use crate::WasmType::I32;

        if signature.params.as_slice() != [I32, I32, I32] {
            return None;
        }
        let FunctionRecord {
            kind: FunctionKind::User(program_id, function_id),
            env,
            ..
        } = self.heap.function(function)?
        else {
            return None;
        };
        let program = self.programs.get(*program_id)?;
        let metadata = program.function_get(*function_id as usize)?;
        if metadata.params != 3
            || metadata.inherited_with_scope
            || !metadata.binding_sites.is_empty()
            || metadata
                .parameter_atoms
                .iter()
                .map(|atom| &program.atoms[*atom as usize])
                .ne(["dest", "src", "num"].iter().copied())
        {
            return None;
        }
        let span = metadata.source_span?;
        let source = program
            .source
            .as_ref()?
            .text
            .get(span.start as usize..span.end as usize)?;
        if source != "(dest, src, num) => HEAPU8.copyWithin(dest, src, src + num)" {
            return None;
        }
        // Free-name atoms need not appear in `environment_atoms` (which describes
        // bindings owned by this function); the complete program atom table includes
        // both local and captured names.
        let view_atom = program
            .atoms
            .iter()
            .position(|atom| atom == "HEAPU8")?
            .try_into()
            .ok()?;
        // The exact source reads one captured name, HEAPU8, so its one capture load names the
        // binding's depth and slot; name bindings are kept only for functions that resolve
        // names dynamically.
        let mut captures = metadata
            .instructions()
            .filter(|instruction| instruction.op() == crate::bytecode::Op::LoadCapture);
        let load = captures.next()?;
        if captures.next().is_some() {
            return None;
        }
        let (depth, slot) = (load.capture_depth(), load.capture_slot());
        let owner_environment = self.environment_capture_env(*env, false, depth)?;

        // Verify that the statically resolved binding still names this slot in a
        // plain lexical environment. Captured with/eval scopes can shadow a static
        // outer declaration even though their parent chain also contains that slot.
        let mut environment = *env;
        loop {
            let Cell::Environment {
                parent,
                function: owner_function,
                slots,
                scope,
            } = self.heap.get(environment)?
            else {
                return None;
            };
            if scope.root_eval_scope()
                || scope.binding_site_pc().is_some()
                || !scope.with_objects().is_empty()
                || self
                    .heap
                    .environment_bindings(environment)?
                    .iter()
                    .any(|(atom, _)| *atom == view_atom)
            {
                return None;
            }
            if environment == owner_environment {
                let owner_program_id = super::program_store::ProgramId::from_raw(scope.program()?);
                let owner_program = self.programs.get(owner_program_id)?;
                let owner_metadata = owner_program.function_get(*owner_function as usize)?;
                let owns_atom = if *owner_function == super::ROOT_FUNCTION_ID {
                    owner_metadata.declares_global_lexical(view_atom)
                        || (owner_program.root_variables_are_local()
                            && owner_metadata.global_var_atoms().contains(&view_atom))
                } else {
                    owner_metadata.declares_environment_atom(view_atom)
                };
                if !owns_atom
                    || self.local_binding_slot(&owner_program, *owner_function, view_atom)?
                        != usize::from(slot)
                    || usize::from(slot) >= slots.len()
                {
                    return None;
                }
                return Some((environment, usize::from(slot)));
            }
            environment = *parent;
        }
    }

    fn wasm_import_global(
        &mut self,
        p: &ResidualProgram,
        module: &crate::WasmModule,
        name: &crate::WasmImportName,
        value: Value,
    ) -> Result<Value, JsError> {
        if matches!(self.heap.get(value), Some(Cell::WasmGlobal { .. })) {
            return Ok(value);
        }
        let (ty, mutable) = module
            .globals
            .iter()
            .find_map(|global| match &global.initial {
                crate::WasmGlobalInitializer::Import { ty, name: import }
                    if import.index == name.index =>
                {
                    Some((*ty, global.mutable))
                }
                _ => None,
            })
            .ok_or_else(|| JsError::validation("invalid Wasm global import".into()))?;
        let numeric =
            value.as_number().is_some() || matches!(self.heap.get(value), Some(Cell::BigInt(_)));
        if mutable || (!numeric && !matches!(ty, WasmType::Reference { .. })) {
            return Err(self.wasm_js_error(
                p,
                Native::WasmLinkError,
                format!(
                    "WebAssembly global import '{}.{}' has the wrong type",
                    name.module, name.name
                ),
            ));
        }
        let declarations = module.signatures.declarations.clone();
        let encoded = self.js_to_wasm(p, value, ty, Some(&declarations))?;
        Ok(self.alloc_wasm_global(encoded, ty, declarations, false))
    }

    fn wasm_import_object(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        name: &crate::WasmImportName,
        accepts: impl Fn(&Cell) -> bool,
    ) -> Result<Value, JsError> {
        if self.heap.get(value).is_some_and(accepts) {
            return Ok(value);
        }
        Err(self.wasm_js_error(
            p,
            Native::WasmLinkError,
            format!(
                "WebAssembly import '{}.{}' has the wrong kind",
                name.module, name.name
            ),
        ))
    }

    /// The frozen, null-prototype exports object of a fresh instance.
    fn wasm_exports_object(
        &mut self,
        p: &ResidualProgram,
        instance: &crate::WasmInstance,
    ) -> Result<Value, JsError> {
        let exports = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        let exports_root = self.heap.root(exports);
        let outcome = (|| {
            for export in instance.module.exports().to_vec() {
                let value = match export.kind {
                    ExternalKind::Func | ExternalKind::FuncExact => {
                        let function = self.wasm_function(instance, export.index)?;
                        self.wasm_js_function(function, Some(export.index))?
                    }
                    ExternalKind::Table => self.wasm_table(instance, export.index)?,
                    ExternalKind::Memory => self.wasm_memory(instance, export.index)?,
                    ExternalKind::Global => self.wasm_global_binding(instance, export.index)?,
                    ExternalKind::Tag => self.wasm_tag(instance, export.index)?,
                };
                let exports = self.heap.root_value(exports_root).expect("rooted exports");
                let key = self
                    .heap
                    .alloc_string(JsString::from_str(&export.name));
                let key = self.to_property_key(p, key)?;
                let record = super::object_descriptors::PropertyDescriptorRecord::data(value);
                self.define_own_property_record(p, exports, key, record)?;
            }
            let exports = self.heap.root_value(exports_root).expect("rooted exports");
            self.set_integrity_level(p, exports, true)
        })();
        self.heap.release_root(exports_root);
        outcome
    }

    /// The one Exported Function wrapping Wasm callable `function` (a Wasm closure or a
    /// host function), made on first request. Its name is the callable's index in the module
    /// exporting it when known.
    pub(super) fn wasm_js_function(
        &mut self,
        function: Value,
        index: Option<u32>,
    ) -> Result<Value, JsError> {
        if function.is_null() {
            return Ok(Value::NULL);
        }
        if let Some(wrapper) = self
            .wasm_exported_functions
            .get(&function)
            .and_then(|handle| self.heap.weak_value(*handle))
        {
            return Ok(wrapper);
        }
        let length = self
            .wasm_callable_signature(function)
            .ok_or_else(|| JsError::validation("not a Wasm callable".into()))?
            .params
            .len();
        let wrapper = self.native_with_env(Native::WasmExportedFunction, function);
        let name = index.map_or_else(String::new, |index| index.to_string());
        self.set_builtin_function_name(wrapper, &name)?;
        self.set_builtin_function_length(wrapper, length as f64)?;
        let handle = self
            .heap
            .weak_handle(wrapper)
            .expect("a fresh wrapper is a heap cell");
        self.wasm_exported_functions.insert(function, handle);
        Ok(wrapper)
    }

    /// The Wasm closure behind an exported-function wrapper.
    fn wasm_exported_closure(&self, value: Value) -> Option<Value> {
        match self.heap.function(value)? {
            FunctionRecord {
                kind: FunctionKind::Native(Native::WasmExportedFunction),
                env,
                ..
            } => Some(*env),
            _ => None,
        }
    }

    /// Drops wrappers the last collection reclaimed.
    pub(super) fn prune_wasm_exported_functions(&mut self) {
        let heap = &self.heap;
        self.wasm_exported_functions
            .retain(|_, handle| heap.weak_value(*handle).is_some());
    }

    #[inline(never)]
    pub(super) fn call_wasm_exported_function(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let function = self
            .active_native_env()
            .ok_or_else(|| JsError::validation("missing Wasm exported function".into()))?;
        let Some(&FunctionRecord {
            kind: FunctionKind::User(program, index),
            env: environment,
            ..
        }) = self.heap.function(function)
        else {
            return self.call_wasm_host_export(p, function, args);
        };
        // The pool is shared, so the signature and declarations stay borrowed across the
        // call instead of being copied per call (JetStream richards-wasm calls a short export
        // two million times an iteration).
        let signatures = self
            .programs
            .wasm_signatures_shared(program)
            .ok_or_else(|| JsError::validation("missing Wasm function signature".into()))?;
        let signature = signatures
            .defined_signature(index)
            .ok_or_else(|| JsError::validation("missing Wasm function signature".into()))?;
        let declarations = Some(&signatures.declarations);
        let mut encoded: smallvec::SmallVec<[Value; WASM_INLINE_ARGUMENTS]> =
            smallvec::SmallVec::with_capacity(signature.params.len());
        if signature.params.iter().any(|ty| encoding_allocates(*ty)) {
            // Every argument converts before any encodes: a conversion can run JavaScript,
            // and an allocated encoding is not rooted.
            let mut values: smallvec::SmallVec<[WasmValue; WASM_INLINE_ARGUMENTS]> =
                smallvec::SmallVec::with_capacity(signature.params.len());
            for (position, &ty) in signature.params.iter().enumerate() {
                let value = args.get(position).copied().unwrap_or(Value::UNDEFINED);
                values.push(self.js_to_wasm_value(p, value, ty, declarations)?);
            }
            encoded.extend(values.into_iter().map(|value| self.encode_wasm_value(value)));
        } else {
            for (position, &ty) in signature.params.iter().enumerate() {
                let value = args.get(position).copied().unwrap_or(Value::UNDEFINED);
                let value = self.js_to_wasm_value(p, value, ty, declarations)?;
                encoded.push(self.encode_wasm_value(value));
            }
        }
        let previous = self.replace_active_program(program);
        let result = match self.run_compiled(program, environment, index, &encoded) {
            Some(result) => result,
            None => match self.programs.get(program) {
                Some(residual) => {
                    // The interpreted call pushes a frame, which observes the frame stack: a
                    // lane's deferred activations become frames first, from their caller's
                    // program, as a call of guest code makes them (`runs_beside_activations`).
                    if self.lane_deferred.is_some() {
                        self.set_active_program(previous);
                        self.materialize_deferred_lane();
                        self.replace_active_program(program);
                    }
                    self.call_user(
                        &residual,
                        index,
                        environment,
                        Value::UNDEFINED,
                        &encoded,
                        CallContext::Internal,
                    )
                }
                None => Err(JsError::validation("missing Wasm program".into())),
            },
        };
        self.set_active_program(previous);
        let result = result.map_err(|error| self.trap_raised_in_javascript(p, error))?;
        match signature.results.as_slice() {
            [] => Ok(Value::UNDEFINED),
            &[ty] => {
                let value = self.decode_wasm_value_in(result, ty, declarations)?;
                self.wasm_value_to_js(p, value, ty)
            }
            types => {
                let results = self.decode_wasm_result_types(types, result, declarations)?;
                self.wasm_results_to_js(p, results, types)
            }
        }
    }

    /// `error`, leaving Wasm for the JavaScript that called an export: a trap gets its
    /// `RuntimeError` here, while its JavaScript callers are on the frame stack (deferred lane
    /// activations become frames first), as V8 captures the stack at the throw; made where a
    /// handler takes it, its stack would name only the handler's frame. It stays a trap
    /// (`JsError::wasm_trap`), which no Wasm handler catches.
    fn trap_raised_in_javascript(&mut self, p: &ResidualProgram, mut error: JsError) -> JsError {
        let Some(trap) = error.wasm_trap().filter(|_| error.thrown_value().is_none()) else {
            return error;
        };
        self.materialize_deferred_lane();
        if let Some(object) = self.wasm_js_error(p, Native::WasmRuntimeError, trap.to_string()).thrown_value() {
            error.replace_thrown_value(object);
        }
        error
    }

    /// `call_wasm_exported_function` for an exported host function, which converts and
    /// checks its own values.
    fn call_wasm_host_export(
        &mut self,
        p: &ResidualProgram,
        function: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let signature = self
            .wasm_callable_signature(function)
            .cloned()
            .ok_or_else(|| JsError::validation("missing Wasm function signature".into()))?;
        let mut values = Vec::with_capacity(signature.params.len());
        for (position, &ty) in signature.params.iter().enumerate() {
            let value = args.get(position).copied().unwrap_or(Value::UNDEFINED);
            values.push(self.js_to_wasm_value(p, value, ty, None)?);
        }
        let encoded = values
            .into_iter()
            .map(|value| self.encode_wasm_value(value))
            .collect::<Vec<_>>();
        let result = self.call_value(p, function, Value::UNDEFINED, &encoded)?;
        let results = self.decode_wasm_result_types(&signature.results, result, None)?;
        self.wasm_results_to_js(p, results, &signature.results)
    }

    /// A Wasm call's results, of types `types`, as JavaScript: undefined, the single value,
    /// or an array.
    fn wasm_results_to_js(
        &mut self,
        p: &ResidualProgram,
        results: Vec<WasmValue>,
        types: &[WasmType],
    ) -> Result<Value, JsError> {
        match (results.as_slice(), types) {
            ([], _) => Ok(Value::UNDEFINED),
            ([value], [ty]) => self.wasm_value_to_js(p, *value, *ty),
            _ => {
                let mut values = Vec::with_capacity(results.len());
                for (value, &ty) in results.into_iter().zip(types) {
                    values.push(self.wasm_value_to_js(p, value, ty)?);
                }
                Ok(self.new_array(values))
            }
        }
    }

    /// Calls the JavaScript callable behind an imported host function with the encoded
    /// Wasm `args`, returning the encoded result.
    pub(super) fn call_wasm_js_import(
        &mut self,
        function: Value,
        memcpy_capture: Option<(Value, usize)>,
        signature: &crate::WasmSignature,
        args: &[Value],
        current_memory: Value,
        memcpy_fast_path_hits: *mut u64,
    ) -> Result<Value, JsError> {
        let program = self
            .programs
            .get(self.active_program)
            .ok_or_else(|| JsError::validation("missing host caller program".into()))?;
        // Only a concrete reference type needs the caller's declarations to convert.
        let declarations = signature
            .params
            .iter()
            .chain(&signature.results)
            .any(|ty| ty.requires_declarations())
            .then(|| self.programs.wasm_signatures(self.active_program))
            .flatten()
            .map(|signatures| signatures.declarations.clone());
        let mut arguments: smallvec::SmallVec<[Value; 8]> = smallvec::SmallVec::new();
        for (&value, &ty) in args.iter().zip(&signature.params) {
            arguments.push(self.wasm_to_js(&program, value, ty, declarations.as_ref())?);
        }
        let result = if let Some((view_environment, view_slot)) = memcpy_capture {
            let view = self
                .heap
                .environment_slot(view_environment, view_slot)
                .unwrap_or(Value::UNDEFINED);
            let copy_within_atom = self.intern_atom("copyWithin");
            let direct_copy = emscripten_memcpy_direct_copy_enabled()
                && self.try_emscripten_memcpy_same_memory(
                    view,
                    current_memory,
                    signature,
                    args,
                    copy_within_atom,
                )?;
            if direct_copy {
                note_emscripten_memcpy_fast_path(memcpy_fast_path_hits);
                Value::UNDEFINED
            } else if self.can_inline_emscripten_memcpy(view, copy_within_atom) {
                let _stack = self.enter_stack()?;
                self.check_execution_interrupt()?;
                let end = Value::number(
                    arguments[1].as_number().unwrap_or_default()
                        + arguments[2].as_number().unwrap_or_default(),
                );
                self.typed_array_native(
                    &program,
                    Native::Uint8ArrayCopyWithin,
                    view,
                    &[arguments[0], arguments[1], end],
                )?
            } else {
                self.call_value(&program, function, Value::UNDEFINED, &arguments)?
            }
        } else {
            self.call_value(&program, function, Value::UNDEFINED, &arguments)?
        };
        match signature.results.as_slice() {
            [] => Ok(Value::UNDEFINED),
            [ty] => self.js_to_wasm(&program, result, *ty, declarations.as_ref()),
            types => {
                let roots = self.iterable_to_rooted_list(&program, result)?;
                let converted = (|| {
                    if roots.len() != types.len() {
                        return Err(self.type_error(
                            &program,
                            "Wasm import returned the wrong number of results".into(),
                        ));
                    }
                    let mut values = Vec::with_capacity(types.len());
                    for (&root, &ty) in roots.iter().zip(types) {
                        let value = self.heap.root_value(root).expect("rooted result");
                        values.push(self.js_to_wasm_value(&program, value, ty, declarations.as_ref())?);
                    }
                    Ok(values)
                })();
                for root in roots {
                    self.heap.release_root(root);
                }
                let encoded = converted?
                    .into_iter()
                    .map(|value| self.encode_wasm_value(value))
                    .collect::<Vec<_>>();
                Ok(self.heap.alloc(Cell::Array {
                    object: crate::heap::ArrayObject::new(
                        Self::empty_object(Value::NULL),
                        encoded,
                    ),
                }))
            }
        }
    }

    /// Copy the canonical Emscripten helper's bytes directly only when its
    /// current captured view is the full, live view of the active unshared
    /// Wasm memory. All other cases retain the existing JS/typed-array path.
    fn try_emscripten_memcpy_same_memory(
        &mut self,
        view: Value,
        current_memory: Value,
        signature: &crate::WasmSignature,
        args: &[Value],
        copy_within_atom: crate::bytecode::Atom,
    ) -> Result<bool, JsError> {
        if signature.params.as_slice() != [WasmType::I32, WasmType::I32, WasmType::I32]
            || !signature.results.is_empty()
            || args.len() != 3
        {
            return Ok(false);
        }

        let mut indices = [0i32; 3];
        for (index, argument) in args.iter().copied().enumerate() {
            let WasmValue::I32(value) = self.decode_wasm_value(argument, WasmType::I32)? else {
                return Ok(false);
            };
            indices[index] = value;
        }
        Ok(self.try_emscripten_memcpy_same_memory_raw(
            view,
            current_memory,
            indices,
            copy_within_atom,
        ))
    }

    /// The compiled callout's opt-in raw-i32 admission path. It reloads the
    /// captured view here on every call and shares W-08's complete memory guard.
    pub(super) fn try_emscripten_memcpy_same_memory_raw_capture(
        &mut self,
        view_environment: Value,
        view_slot: usize,
        current_memory: Value,
        indices: [i32; 3],
        raw_i32_fast_path_hits: *mut u64,
    ) -> bool {
        let view = self
            .heap
            .environment_slot(view_environment, view_slot)
            .unwrap_or(Value::UNDEFINED);
        let copy_within_atom = self.intern_atom("copyWithin");
        let hit = self.try_emscripten_memcpy_same_memory_raw(
            view,
            current_memory,
            indices,
            copy_within_atom,
        );
        if hit {
            note_emscripten_memcpy_raw_i32_hit(raw_i32_fast_path_hits);
        }
        hit
    }

    fn try_emscripten_memcpy_same_memory_raw(
        &mut self,
        view: Value,
        current_memory: Value,
        indices: [i32; 3],
        copy_within_atom: crate::bytecode::Atom,
    ) -> bool {
        if !self.can_inline_emscripten_memcpy(view, copy_within_atom) {
            return false;
        }

        let (view_buffer, view_offset) = match self.heap.get(view) {
            Some(Cell::TypedArray {
                buffer,
                kind: crate::heap::TypedArrayKind::Uint8,
                object,
                ..
            }) => (*buffer, object.offset),
            _ => return false,
        };
        if view_offset != 0 {
            return false;
        }

        let (memory_bytes, memory_buffer) = match self.heap.get(current_memory) {
            Some(Cell::WasmMemory { bytes, buffer, .. }) => (bytes, *buffer),
            _ => return false,
        };
        if memory_buffer.is_undefined() || view_buffer != memory_buffer {
            return false;
        }

        let buffer_bytes = match self.heap.get(view_buffer) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared: false,
                detached: false,
                immutable: false,
                ..
            }) if bytes.is_wasm_view_of(memory_bytes) => bytes,
            _ => return false,
        };
        let Some((base, memory_length)) =
            super::dispatch_fast::unshared_memory(self, current_memory)
        else {
            return false;
        };
        if self.typed_array_length(view) != Some(memory_length)
            || buffer_bytes.len() != memory_length
        {
            return false;
        }
        let target = copy_within_relative_index(i64::from(indices[0]), memory_length);
        let start = copy_within_relative_index(i64::from(indices[1]), memory_length);
        let raw_end = i64::from(indices[1]) + i64::from(indices[2]);
        let end = copy_within_relative_index(raw_end, memory_length);
        let count = end.saturating_sub(start).min(memory_length - target);
        if count > 0 {
            // SAFETY: the active memory is unshared; the current live view
            // names that exact allocation, and target/start/count are clamped
            // to its current length. The compiled caller is suspended here.
            unsafe {
                std::ptr::copy(base.add(start), base.add(target), count);
            }
        }
        true
    }

    /// Whether the exact Emscripten helper call can be inlined without running any
    /// JavaScript during property lookup. Accessors, proxies and customized methods
    /// stay on the ordinary JS call path, which also preserves their caller frames.
    fn can_inline_emscripten_memcpy(
        &self,
        view: Value,
        copy_within_atom: crate::bytecode::Atom,
    ) -> bool {
        let Some(Cell::TypedArray { buffer, kind, .. }) = self.heap.get(view) else {
            return false;
        };
        if *kind != crate::heap::TypedArrayKind::Uint8
            || self.typed_array_out_of_bounds(view)
            || self.array_buffer_detached(*buffer)
            || matches!(
                self.heap.get(*buffer),
                Some(Cell::ArrayBuffer {
                    immutable: true,
                    ..
                })
            )
        {
            return false;
        }
        self.data_property_without_getter(view, copy_within_atom)
            .is_some_and(|method| {
                matches!(
                    self.heap.function(method),
                    Some(FunctionRecord {
                        kind: FunctionKind::Native(Native::Uint8ArrayCopyWithin),
                        ..
                    })
                )
            })
    }

    /// Looks up an own data property through ordinary object prototypes without
    /// invoking accessors. A proxy or any accessor makes the result unknown.
    fn data_property_without_getter(
        &self,
        object: Value,
        atom: crate::bytecode::Atom,
    ) -> Option<Value> {
        let key = PropertyKey::string(atom);
        let mut holder = object;
        loop {
            if matches!(self.heap.get(holder), Some(Cell::Proxy { .. })) {
                return None;
            }
            if let Some(attributes) = self.property_attributes(holder, key) {
                if attributes.accessor {
                    return None;
                }
                return self.own_property(holder, atom);
            }
            holder = self.object_data(holder)?.proto();
            if holder.is_null() {
                return None;
            }
        }
    }

    /// Whether `value` is a Wasm GC object (a struct or array reference) in JavaScript: an
    /// exotic object with a null prototype and no properties, which every mutation refuses.
    pub(super) fn is_wasm_gc_object(&self, value: Value) -> bool {
        matches!(self.heap.get(value), Some(Cell::WasmGc { .. }))
    }

    /// The TypeError of a mutation of a Wasm GC object (V8: "WebAssembly objects are opaque").
    pub(super) fn wasm_gc_opaque_error(&mut self, p: &ResidualProgram) -> JsError {
        self.type_error(p, "WebAssembly objects are opaque".into())
    }

    /// ToJSValue of an encoded Wasm value of type `ty`.
    fn wasm_to_js(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        ty: WasmType,
        declarations: Option<&crate::WasmTypes>,
    ) -> Result<Value, JsError> {
        let value = self.decode_wasm_value_in(value, ty, declarations)?;
        self.wasm_value_to_js(p, value, ty)
    }

    /// ToJSValue of `value`, of type `ty`. An internal reference leaves Wasm as `extern.convert_any`
    /// would: null, an `i31` as its signed number, an internalized host value as itself. An
    /// exception reference has no JavaScript representation.
    fn wasm_value_to_js(
        &mut self,
        p: &ResidualProgram,
        value: WasmValue,
        ty: WasmType,
    ) -> Result<Value, JsError> {
        Ok(match value {
            WasmValue::I32(value) => Value::number(f64::from(value)),
            WasmValue::I64(value) => self.heap.alloc(Cell::bigint(value.into())),
            WasmValue::F32(bits) => Value::number(f64::from(f32::from_bits(bits))),
            WasmValue::F64(bits) => Value::number(f64::from_bits(bits)),
            WasmValue::FuncRef(function) => self.wasm_js_function(function, None)?,
            WasmValue::ExternRef(value) => self.wasm_external_value(value),
            WasmValue::GcRef(value) if !ty.is_exception_reference() => {
                self.wasm_external_value(value)
            }
            WasmValue::V128(_) | WasmValue::GcRef(_) => {
                return Err(self.type_error(
                    p,
                    "this Wasm value type has no JavaScript representation".into(),
                ));
            }
        })
    }

    /// ToWebAssemblyValue of `value` for type `ty`, encoded.
    fn js_to_wasm(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        ty: WasmType,
        declarations: Option<&crate::WasmTypes>,
    ) -> Result<Value, JsError> {
        let value = self.js_to_wasm_value(p, value, ty, declarations)?;
        Ok(self.encode_wasm_value(value))
    }

    /// ToWebAssemblyValue of `value` for type `ty`; `declarations` own a concrete GC type.
    fn js_to_wasm_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        ty: WasmType,
        declarations: Option<&crate::WasmTypes>,
    ) -> Result<WasmValue, JsError> {
        Ok(match ty {
            WasmType::I32 => {
                let number = self.to_number(p, value)?;
                WasmValue::I32(Self::uint32_from_value(number) as i32)
            }
            WasmType::I64 => {
                let value = self.to_bigint(p, value)?;
                let mut bytes = [if value.sign() == num_bigint::Sign::Minus {
                    u8::MAX
                } else {
                    0
                }; size_of::<i64>()];
                for (target, source) in bytes.iter_mut().zip(value.to_signed_bytes_le()) {
                    *target = source;
                }
                WasmValue::I64(i64::from_le_bytes(bytes))
            }
            WasmType::F32 => WasmValue::F32((self.to_number(p, value)? as f32).to_bits()),
            WasmType::F64 => WasmValue::F64(self.to_number(p, value)?.to_bits()),
            // A function reference is null or an exported Wasm function's closure, of the type.
            WasmType::Reference {
                kind:
                    crate::WasmReferenceKind::Function
                    | crate::WasmReferenceKind::DeclaredFunction { .. }
                    | crate::WasmReferenceKind::BottomFunction,
                ..
            } => {
                let closure = if value.is_null() {
                    Some(Value::NULL)
                } else {
                    self.wasm_exported_closure(value)
                };
                match closure {
                    Some(closure) if self.wasm_reference_valid_in(closure, ty, declarations) => {
                        WasmValue::FuncRef(closure)
                    }
                    _ => {
                        return Err(self.type_error(
                            p,
                            "value must be null or an exported WebAssembly function of the type"
                                .into(),
                        ));
                    }
                }
            }
            WasmType::Reference {
                kind: crate::WasmReferenceKind::External | crate::WasmReferenceKind::BottomExternal,
                ..
            } => {
                if !self.wasm_reference_valid_in(value, ty, declarations) {
                    return Err(self.type_error(
                        p,
                        "value does not have the Wasm reference type".into(),
                    ));
                }
                WasmValue::ExternRef(value)
            }
            // An internal reference enters as `any.convert_extern` would take it (null, an
            // i31-range integer, a Wasm GC object as itself, any other value boxed), and
            // must then have the type.
            WasmType::Reference {
                kind: crate::WasmReferenceKind::Internal(_) | crate::WasmReferenceKind::DeclaredGc { .. },
                ..
            } if !ty.is_exception_reference() => {
                let internal = self.wasm_external_conversion(
                    crate::wasm::reference::ExternalConversion::Internalize,
                    value,
                );
                if !self.wasm_reference_valid_in(internal, ty, declarations) {
                    return Err(self.type_error(
                        p,
                        "value does not have the Wasm reference type".into(),
                    ));
                }
                WasmValue::GcRef(internal)
            }
            _ => {
                return Err(self.type_error(
                    p,
                    "this Wasm value type has no JavaScript conversion".into(),
                ));
            }
        })
    }

    fn wasm_descriptor(
        &mut self,
        p: &ResidualProgram,
        fields: &[(&str, &str)],
    ) -> Result<Value, JsError> {
        let descriptor = self.object();
        for &(name, text) in fields {
            let value = self.heap.alloc_string(JsString::from_str(text));
            self.set_named(p, descriptor, name, value)?;
        }
        Ok(descriptor)
    }

    fn wasm_descriptor_object(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        if self.is_object_like(value) {
            Ok(value)
        } else {
            Err(self.type_error(p, "WebAssembly descriptor must be an object".into()))
        }
    }

    fn wasm_descriptor_member(
        &mut self,
        p: &ResidualProgram,
        descriptor: Value,
        name: &str,
    ) -> Result<Value, JsError> {
        let atom = self.intern_atom(name);
        self.get_property(p, descriptor, atom)
    }

    /// A WebIDL `[EnforceRange] unsigned long`.
    fn wasm_descriptor_u32(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        name: &str,
    ) -> Result<u32, JsError> {
        let number = self.to_number(p, value)?;
        let integer = number.trunc();
        if !number.is_finite() || !(0.0..=DESCRIPTOR_LIMIT).contains(&integer) {
            return Err(self.type_error(
                p,
                format!("WebAssembly '{name}' must be an unsigned 32-bit integer"),
            ));
        }
        Ok(integer as u32)
    }

    fn wasm_memory_receiver(&mut self, p: &ResidualProgram, this: Value) -> Result<(), JsError> {
        match self.heap.get(this) {
            Some(Cell::WasmMemory { .. }) => Ok(()),
            _ => Err(self.type_error(p, "receiver is not a WebAssembly.Memory".into())),
        }
    }

    /// The length of table receiver `this`.
    fn wasm_table_receiver(&mut self, p: &ResidualProgram, this: Value) -> Result<usize, JsError> {
        match self.heap.get(this) {
            Some(Cell::WasmTable { elements, .. }) => Ok(elements.len()),
            _ => Err(self.type_error(p, "receiver is not a WebAssembly.Table".into())),
        }
    }

    fn wasm_js_table_index(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        length: usize,
    ) -> Result<usize, JsError> {
        let index = self.wasm_descriptor_u32(p, value, "index")? as usize;
        if index >= length {
            return Err(self.range_error(p, "WebAssembly.Table index is out of bounds".into()));
        }
        Ok(index)
    }

    /// The declarations of the module that defined `table`'s element type.
    fn wasm_table_declarations(&self, table: Value) -> Option<crate::WasmTypes> {
        match self.heap.get(table) {
            Some(Cell::WasmTable { declarations, .. }) => Some((**declarations).clone()),
            _ => None,
        }
    }

    /// The declarations of the module that defined `global`'s type.
    fn wasm_global_declarations(&self, global: Value) -> Option<crate::WasmTypes> {
        match self.heap.get(global) {
            Some(Cell::WasmGlobal { header, .. }) => Some(header.declarations.clone()),
            _ => None,
        }
    }

    fn wasm_table_element_type(&self, table: Value) -> WasmType {
        match self.heap.get(table) {
            Some(Cell::WasmTable { element_type, declarations, .. }) => {
                reference_value_type_in(*element_type, declarations)
            }
            _ => unreachable!("checked table receiver"),
        }
    }

    /// The value a table slot takes when no initial value is given (DefaultValue).
    fn wasm_table_default(&self, ty: WasmType) -> Value {
        match ty {
            WasmType::Reference {
                kind: crate::WasmReferenceKind::External,
                ..
            } => Value::UNDEFINED,
            _ => Value::NULL,
        }
    }

    /// The zero value a global takes when no initial value is given.
    fn wasm_global_default(&mut self, ty: WasmType) -> Value {
        match ty {
            WasmType::I32 => self.encode_wasm_value(WasmValue::I32(0)),
            WasmType::I64 => self.encode_wasm_value(WasmValue::I64(0)),
            WasmType::F32 => self.encode_wasm_value(WasmValue::F32(0)),
            WasmType::F64 => self.encode_wasm_value(WasmValue::F64(0)),
            other => self.wasm_table_default(other),
        }
    }

    fn wasm_global_receiver(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<(Value, WasmType, bool), JsError> {
        match self.heap.get(this) {
            Some(Cell::WasmGlobal {
                value, ty, mutable, ..
            }) => Ok((*value, *ty, *mutable)),
            _ => Err(self.type_error(p, "receiver is not a WebAssembly.Global".into())),
        }
    }
}

/// `WebAssembly.JSTag`'s type: a tag whose one `externref` parameter carries a JavaScript
/// exception through Wasm. Every realm makes the same one.
pub(crate) fn js_tag_type() -> (crate::WasmTypes, u32) {
    let declarations = crate::WasmTypes::from_functions([wasmparser::FuncType::new(
        [wasmparser::ValType::EXTERNREF],
        [],
    )]);
    (declarations, 0)
}

/// `WebAssembly.JSTag`'s cell (`js_tag_type`).
pub(crate) fn js_tag_cell() -> Cell {
    let (declarations, ty) = js_tag_type();
    Cell::WasmTag { declarations, ty }
}
