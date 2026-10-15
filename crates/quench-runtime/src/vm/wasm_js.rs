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

/// The main realm's interface prototypes, which every new Wasm object of the matching kind
/// receives. Null until the namespace is installed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WasmJsIntrinsics {
    pub(crate) module: Value,
    pub(crate) instance: Value,
    pub(crate) memory: Value,
    pub(crate) table: Value,
    pub(crate) global: Value,
}

impl Default for WasmJsIntrinsics {
    fn default() -> Self {
        Self {
            module: Value::NULL,
            instance: Value::NULL,
            memory: Value::NULL,
            table: Value::NULL,
            global: Value::NULL,
        }
    }
}

impl WasmJsIntrinsics {
    pub(crate) fn roots(&self) -> [Value; 5] {
        [
            self.module,
            self.instance,
            self.memory,
            self.table,
            self.global,
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
        "anyfunc" | "funcref" => wasmparser::RefType::FUNCREF,
        "externref" => wasmparser::RefType::EXTERNREF,
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
    WasmType::Reference {
        kind: if element_type.heap_type() == wasmparser::HeapType::FUNC {
            crate::WasmReferenceKind::Function
        } else {
            crate::WasmReferenceKind::External
        },
        nullable: element_type.is_nullable(),
    }
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
                self.set_property(prototype, atom, Value::UNDEFINED)?;
                self.set_property_attributes(
                    prototype,
                    PropertyKey::string(atom),
                    PropertyAttributes {
                        writable: false,
                        enumerable: true,
                        configurable: true,
                        accessor: true,
                        getter: Some(getter),
                        setter,
                    },
                );
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
            self.set_builtin_value_named(namespace, name, self.native_value(native))?;
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
        let message = self.heap.alloc(Cell::String(JsString::from_str(&text)));
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
                self.wasm_to_js(p, value, ty)
            }
            Native::WasmTableSet => {
                let length = self.wasm_table_receiver(p, this)?;
                let index = self.wasm_js_table_index(p, arg(0), length)?;
                let ty = self.wasm_table_element_type(this);
                let value = match args.get(1) {
                    Some(value) => self.js_to_wasm(p, *value, ty)?,
                    None => self.wasm_table_default(ty),
                };
                if let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(this) {
                    elements[index] = value;
                }
                Ok(Value::UNDEFINED)
            }
            Native::WasmTableGrow => {
                let length = self.wasm_table_receiver(p, this)?;
                let delta = self.wasm_descriptor_u32(p, arg(0), "delta")?;
                let ty = self.wasm_table_element_type(this);
                let value = match args.get(1) {
                    Some(value) => self.js_to_wasm(p, *value, ty)?,
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
                Ok(Value::number(length as f64))
            }
            Native::WasmGlobalValueGetter | Native::WasmGlobalValueOf => {
                let (value, ty, _) = self.wasm_global_receiver(p, this)?;
                self.wasm_to_js(p, value, ty)
            }
            Native::WasmGlobalValueSetter => {
                let (_, ty, mutable) = self.wasm_global_receiver(p, this)?;
                if !mutable {
                    return Err(self.type_error(
                        p,
                        "cannot set the value of an immutable WebAssembly.Global".into(),
                    ));
                }
                let value = self.js_to_wasm(p, arg(0), ty)?;
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
                    Some(value) if !value.is_undefined() => self.js_to_wasm(p, *value, ty)?,
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
                    Some(value) if !value.is_undefined() => self.js_to_wasm(p, *value, ty)?,
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
        let promise = self.promise_object();
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
        self.create_wasm_host_function(&name.name, WasmHostTarget::Js(value), signature)
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
        let encoded = self.js_to_wasm(p, value, ty)?;
        Ok(self.alloc_wasm_global(encoded, ty, crate::WasmTypes::default(), false))
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
                    .alloc(Cell::String(JsString::from_str(&export.name)));
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
        match self.heap.get(value)? {
            Cell::Function {
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

    fn call_wasm_exported_function(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let function = self
            .active_native_env()
            .ok_or_else(|| JsError::validation("missing Wasm exported function".into()))?;
        let signature = self
            .wasm_callable_signature(function)
            .cloned()
            .ok_or_else(|| JsError::validation("missing Wasm function signature".into()))?;
        let mut values = Vec::with_capacity(signature.params.len());
        for (position, &ty) in signature.params.iter().enumerate() {
            let value = args.get(position).copied().unwrap_or(Value::UNDEFINED);
            values.push(self.js_to_wasm_value(p, value, ty)?);
        }
        let encoded = values
            .into_iter()
            .map(|value| self.encode_wasm_value(value))
            .collect::<Vec<_>>();
        let (result, declarations) = match self.heap.get(function) {
            Some(Cell::Function {
                kind: FunctionKind::User(program, index),
                env,
                ..
            }) => {
                let (program, index, environment) = (*program, *index, *env);
                let residual = self
                    .programs
                    .get(program)
                    .ok_or_else(|| JsError::validation("missing Wasm program".into()))?;
                let previous = self.replace_active_program(program);
                let result = match self.run_compiled(program, environment, index, &encoded) {
                    Some(result) => result,
                    None => self.call_user(
                        &residual,
                        index,
                        environment,
                        Value::UNDEFINED,
                        &encoded,
                        CallContext::Internal,
                    ),
                };
                self.set_active_program(previous);
                let declarations = self
                    .programs
                    .wasm_signatures(program)
                    .map(|signatures| signatures.declarations.clone());
                (result?, declarations)
            }
            // A host function converts and checks its own values.
            _ => (
                self.call_value(p, function, Value::UNDEFINED, &encoded)?,
                None,
            ),
        };
        let results =
            self.decode_wasm_result_types(&signature.results, result, declarations.as_ref())?;
        self.wasm_results_to_js(p, results)
    }

    /// A Wasm call's results as JavaScript: undefined, the single value, or an array.
    fn wasm_results_to_js(
        &mut self,
        p: &ResidualProgram,
        results: Vec<WasmValue>,
    ) -> Result<Value, JsError> {
        match results.as_slice() {
            [] => Ok(Value::UNDEFINED),
            [value] => self.wasm_value_to_js(p, *value),
            _ => {
                let mut values = Vec::with_capacity(results.len());
                for value in results {
                    values.push(self.wasm_value_to_js(p, value)?);
                }
                Ok(self.new_array(values))
            }
        }
    }

    /// Calls the JavaScript callable behind an imported host function with the encoded
    /// Wasm `args`, returning the encoded result.
    pub(super) fn call_wasm_js_import(
        &mut self,
        target: Value,
        signature: &crate::WasmSignature,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let program = self
            .programs
            .get(self.active_program)
            .ok_or_else(|| JsError::validation("missing host caller program".into()))?;
        let mut arguments = Vec::with_capacity(args.len());
        for (&value, &ty) in args.iter().zip(&signature.params) {
            arguments.push(self.wasm_to_js(&program, value, ty)?);
        }
        let result = self.call_value(&program, target, Value::UNDEFINED, &arguments)?;
        match signature.results.as_slice() {
            [] => Ok(Value::UNDEFINED),
            [ty] => self.js_to_wasm(&program, result, *ty),
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
                        values.push(self.js_to_wasm_value(&program, value, ty)?);
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
                        encoded.into(),
                    ),
                }))
            }
        }
    }

    /// ToJSValue of an encoded Wasm value of type `ty`.
    fn wasm_to_js(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        ty: WasmType,
    ) -> Result<Value, JsError> {
        let value = self.decode_wasm_value(value, ty)?;
        self.wasm_value_to_js(p, value)
    }

    fn wasm_value_to_js(
        &mut self,
        p: &ResidualProgram,
        value: WasmValue,
    ) -> Result<Value, JsError> {
        Ok(match value {
            WasmValue::I32(value) => Value::number(f64::from(value)),
            WasmValue::I64(value) => self.heap.alloc(Cell::BigInt(value.to_string())),
            WasmValue::F32(bits) => Value::number(f64::from(f32::from_bits(bits))),
            WasmValue::F64(bits) => Value::number(f64::from_bits(bits)),
            WasmValue::FuncRef(function) => self.wasm_js_function(function, None)?,
            WasmValue::ExternRef(value) => self.wasm_external_value(value),
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
    ) -> Result<Value, JsError> {
        let value = self.js_to_wasm_value(p, value, ty)?;
        Ok(self.encode_wasm_value(value))
    }

    fn js_to_wasm_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        ty: WasmType,
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
            WasmType::Reference {
                kind: crate::WasmReferenceKind::Function,
                nullable,
            } => {
                if value.is_null() && nullable {
                    WasmValue::FuncRef(Value::NULL)
                } else if let Some(closure) = self.wasm_exported_closure(value) {
                    WasmValue::FuncRef(closure)
                } else {
                    return Err(self.type_error(
                        p,
                        "value must be null or an exported WebAssembly function".into(),
                    ));
                }
            }
            WasmType::Reference {
                kind: crate::WasmReferenceKind::External,
                nullable,
            } => {
                if value.is_null() && !nullable {
                    return Err(
                        self.type_error(p, "a non-nullable externref cannot be null".into())
                    );
                }
                WasmValue::ExternRef(value)
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
            let value = self.heap.alloc(Cell::String(JsString::from_str(text)));
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

    fn wasm_table_element_type(&self, table: Value) -> WasmType {
        match self.heap.get(table) {
            Some(Cell::WasmTable { element_type, .. }) => reference_value_type(*element_type),
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
