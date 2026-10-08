//! Wast lifecycle on the shared VM. No legacy execution or scoring policy lives here.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use quench_runtime::{
    Host, RootId, Runtime, WasmHostFunctionId, WasmHostValue, WasmInstance, WasmSignature,
    WasmType, WasmValue,
};
use wasmparser::{Encoding, ExternalKind, Parser, Payload, WasmFeatures};
use wast::core::{WastArgCore, WastRetCore};
use wast::{QuoteWat, WastArg, WastExecute, WastInvoke};

use crate::wast_protocol::{LinkError, Outcome};
use crate::{Module, WastReport};

pub(crate) fn run_wast(filename: &str, source: &str) -> WastReport {
    crate::wast_script::run_wast_using(filename, source, Store::new())
}

const SPECTEST_FUNCTIONS: &[(&str, &[WasmType])] = &[
    ("print", &[]),
    ("print_i32", &[WasmType::I32]),
    ("print_i64", &[WasmType::I64]),
    ("print_f32", &[WasmType::F32]),
    ("print_f64", &[WasmType::F64]),
    ("print_i32_f32", &[WasmType::I32, WasmType::F32]),
    ("print_f64_f64", &[WasmType::F64, WasmType::F64]),
];

struct ScriptHost;
impl Host for ScriptHost {
    fn write_line(&mut self, text: &str) {
        println!("{text}");
    }
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
    fn call_wasm(
        &mut self,
        id: WasmHostFunctionId,
        args: &[WasmHostValue],
    ) -> Result<Vec<WasmHostValue>, String> {
        let (name, _) = SPECTEST_FUNCTIONS
            .get(id.0 as usize)
            .ok_or("unknown spectest host function")?;
        let arguments = args
            .iter()
            .map(|value| match value {
                WasmHostValue::I32(value) => Ok(value.to_string()),
                WasmHostValue::I64(value) => Ok(value.to_string()),
                WasmHostValue::F32(bits) => Ok(f32::from_bits(*bits).to_string()),
                WasmHostValue::F64(bits) => Ok(f64::from_bits(*bits).to_string()),
                _ => Err("spectest print expects numeric arguments"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.write_line(&format!("{name}({})", arguments.join(", ")));
        Ok(vec![])
    }
}

#[derive(Clone, Copy)]
enum Export {
    Wasm(ExternalKind, u32),
    Host(RootId),
}

// A failed replacement stays current: following invokes must never fall back
// to the last successful module. Named aliases share the same instance state.
enum Instance {
    Ready {
        module: Rc<Module>,
        exports: HashMap<String, Export>,
        lowered: Option<WasmInstance>,
    },
    Unsupported(String),
}
type InstanceRef = Rc<RefCell<Instance>>;

struct Definition {
    name: Option<String>,
    module: Result<Rc<Module>, String>,
}

pub(crate) struct Store {
    runtime: Runtime<ScriptHost>,
    current: Option<InstanceRef>,
    named: HashMap<String, InstanceRef>,
    definitions: Vec<Definition>,
    host_references: HashMap<u32, RootId>,
}

// Standard core testsuite host table contract; this is fixture setup only.
const SPECTEST_INTEGER_GLOBAL: i64 = 666;
const SPECTEST_FLOAT_GLOBAL: f64 = 666.6;
const SPECTEST_MEMORY_MIN: u32 = 1;
const SPECTEST_MEMORY_MAX: u32 = 2;
const SPECTEST_TABLE_MIN: u32 = 10;
const SPECTEST_TABLE_MAX: u32 = 20;

impl Store {
    fn new() -> Self {
        let mut store = Self {
            runtime: Runtime::new(ScriptHost),
            current: None,
            named: HashMap::new(),
            definitions: Vec::new(),
            host_references: HashMap::new(),
        };
        let source = format!(
            "(module (table (export \"table\") {SPECTEST_TABLE_MIN} {SPECTEST_TABLE_MAX} funcref) (table (export \"table64\") i64 {SPECTEST_TABLE_MIN} {SPECTEST_TABLE_MAX} funcref) (memory (export \"memory\") {SPECTEST_MEMORY_MIN} {SPECTEST_MEMORY_MAX}) (memory (export \"shared_memory\") {SPECTEST_MEMORY_MIN} {SPECTEST_MEMORY_MAX} shared) (global (export \"global_i32\") i32 (i32.const {SPECTEST_INTEGER_GLOBAL})) (global (export \"global_i64\") i64 (i64.const {SPECTEST_INTEGER_GLOBAL})) (global (export \"global_f32\") f32 (f32.const {SPECTEST_FLOAT_GLOBAL})) (global (export \"global_f64\") f64 (f64.const {SPECTEST_FLOAT_GLOBAL})))"
        );
        let buffer =
            wast::parser::ParseBuffer::new(&source).expect("static spectest resource text");
        let mut wat: wast::Wat<'_> =
            wast::parser::parse(&buffer).expect("static spectest resource module");
        let bytes = wat.encode().expect("static spectest resource encoding");
        // Host resources include both memory kinds. This fixture's features do
        // not broaden the independently validated feature set of guest modules.
        assert_eq!(
            crate::inspect_binary_with(&bytes, crate::core_features() | WasmFeatures::THREADS),
            crate::ModuleStatus::Valid
        );
        let module = Module { bytes };
        let mut instance = store.prepare(Rc::new(module)).unwrap_or_else(|_| {
            panic!("static spectest resources must instantiate on the shared VM")
        });
        let Instance::Ready { exports, .. } = &mut instance else {
            unreachable!()
        };
        for (index, (name, params)) in SPECTEST_FUNCTIONS.iter().enumerate() {
            let function = store
                .runtime
                .wasm_host_function(
                    name,
                    WasmHostFunctionId(index as u32),
                    WasmSignature {
                        params: params.to_vec(),
                        results: vec![],
                    },
                )
                .expect("static spectest host function");
            exports.insert((*name).into(), Export::Host(function));
        }
        store
            .named
            .insert("spectest".into(), Rc::new(RefCell::new(instance)));
        store
    }

    fn prepare(&mut self, module: Rc<Module>) -> Result<Instance, LinkError> {
        let mut has_instance_state = false;
        let mut declaration_types = quench_runtime::WasmTypes::default();
        let mut exports = HashMap::new();
        for payload in Parser::new(0).parse_all(module.bytes()) {
            match payload.map_err(|error| LinkError::Unsupported(error.to_string()))? {
                Payload::Version {
                    encoding: Encoding::Module,
                    ..
                }
                | Payload::FunctionSection(_)
                | Payload::CodeSectionStart { .. }
                | Payload::CodeSectionEntry(_)
                | Payload::DataCountSection { .. }
                | Payload::CustomSection(_)
                | Payload::End(_) => {}
                Payload::TypeSection(reader) => {
                    declaration_types = quench_runtime::WasmTypes::from_groups(
                        reader
                            .into_iter()
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|error| LinkError::Unsupported(error.to_string()))?,
                    );
                }
                Payload::ElementSection(reader) => {
                    for element in reader {
                        let element =
                            element.map_err(|error| LinkError::Unsupported(error.to_string()))?;
                        has_instance_state |=
                            !matches!(element.kind, wasmparser::ElementKind::Declared);
                    }
                }
                Payload::GlobalSection(reader) if !has_instance_state => {
                    // Preserve initializer admission on the lazy, stateless path.
                    crate::shared::decode_globals(reader, &mut Vec::new(), &declaration_types)
                        .map_err(|error| LinkError::Unsupported(error.to_string()))?;
                }
                // Stateful modules lower all declarations together, including imports.
                Payload::GlobalSection(_) => {}
                Payload::MemorySection(_)
                | Payload::TableSection(_)
                | Payload::ImportSection(_)
                | Payload::TagSection(_) => has_instance_state = true,
                Payload::DataSection(_) | Payload::StartSection { .. } => has_instance_state = true,
                Payload::ExportSection(reader) => {
                    for export in reader {
                        let export =
                            export.map_err(|error| LinkError::Unsupported(error.to_string()))?;
                        exports.insert(
                            export.name.to_owned(),
                            Export::Wasm(export.kind, export.index),
                        );
                    }
                }
                _ => {
                    return Err(LinkError::Unsupported(
                        "unsupported shared Wasm module state, imports or section".into(),
                    ));
                }
            }
        }
        // State and start functions are instantiated now, not on first invoke.
        // Keep the preexisting lazy path for modules without memory, table or segment state until their
        // remaining unsupported operators are ported.
        let lowered = if has_instance_state {
            let definition = module
                .lower_shared_module()
                .map_err(|error| LinkError::Unsupported(error.to_string()))?;
            Some(self.instantiate_definition(&definition)?)
        } else {
            None
        };
        Ok(Instance::Ready {
            module,
            exports,
            lowered,
        })
    }

    fn instantiate_definition(
        &mut self,
        definition: &quench_runtime::WasmModule,
    ) -> Result<WasmInstance, LinkError> {
        let mut roots = Vec::new();
        let result = (|| {
            for name in definition.imports() {
                let source = self.named.get(&name.module).cloned().ok_or_else(|| {
                    LinkError::Unlinkable(format!("unknown import module: {}", name.module))
                })?;
                let mut source = source.borrow_mut();
                let Instance::Ready {
                    module,
                    exports,
                    lowered,
                } = &mut *source
                else {
                    return Err(LinkError::Unlinkable(
                        "import module has no valid instance".into(),
                    ));
                };
                let Some(&export) = exports.get(&name.name) else {
                    return Err(LinkError::Unlinkable(format!(
                        "unknown import: {}",
                        name.name
                    )));
                };
                if let Export::Host(root) = export {
                    let Some(value) = self.runtime.rooted_value(root) else {
                        return Err(LinkError::Unlinkable("invalid host function root".into()));
                    };
                    roots.push(self.runtime.root(value));
                    continue;
                }
                let Export::Wasm(kind, index) = export else {
                    unreachable!()
                };
                let instance = Self::instantiate_bindings(&mut self.runtime, module, lowered)
                    .map_err(LinkError::Unsupported)?;
                let value = match kind {
                    ExternalKind::Func => self.runtime.wasm_function(instance, index),
                    ExternalKind::Table => self.runtime.wasm_table(instance, index),
                    ExternalKind::Memory => self.runtime.wasm_memory(instance, index),
                    ExternalKind::Global => self.runtime.wasm_global_binding(instance, index),
                    ExternalKind::Tag => self.runtime.wasm_tag(instance, index),
                    _ => return Err(LinkError::Unlinkable("incompatible import type".into())),
                }
                .map_err(|error| LinkError::Unlinkable(error.to_string()))?;
                roots.push(self.runtime.root(value));
            }
            self.runtime
                .instantiate_wasm_module_with_imports(definition, &roots)
                .map_err(|error| {
                    if error.wasm_exception().is_some() {
                        LinkError::Exception
                    } else if error.wasm_trap().is_some() {
                        LinkError::Trap(error.to_string())
                    } else if error.wasm_link_error().is_some() {
                        LinkError::Unlinkable(error.to_string())
                    } else {
                        LinkError::Unsupported(error.to_string())
                    }
                })
        })();
        for root in roots {
            self.runtime.release_root(root);
        }
        result
    }

    fn get_global(&mut self, module_name: Option<&str>, global_name: &str) -> Outcome<WasmValue> {
        let instance = match module_name {
            Some(name) => self.named.get(name),
            None => self.current.as_ref(),
        };
        let Some(instance) = instance else {
            return Outcome::Missing;
        };
        let mut instance = instance.borrow_mut();
        let Instance::Ready {
            module,
            exports,
            lowered,
        } = &mut *instance
        else {
            let Instance::Unsupported(error) = &*instance else {
                unreachable!()
            };
            return Outcome::Unimplemented(error.clone());
        };
        let Some(&Export::Wasm(ExternalKind::Global, index)) = exports.get(global_name) else {
            return Outcome::Missing;
        };
        let instance = match Self::instantiate_bindings(&mut self.runtime, module, lowered) {
            Ok(instance) => instance,
            Err(error) => return Outcome::Unimplemented(error),
        };
        match self.runtime.wasm_global(instance, index) {
            Ok(value) => Outcome::Values(vec![value]),
            Err(error) => Outcome::Unimplemented(error.to_string()),
        }
    }

    fn instantiate_bindings<'a>(
        runtime: &mut Runtime<ScriptHost>,
        module: &Module,
        lowered: &'a mut Option<WasmInstance>,
    ) -> Result<&'a WasmInstance, String> {
        if lowered.is_none() {
            let lowered_module = module
                .lower_shared_module()
                .map_err(|error| error.to_string())?;
            *lowered = Some(
                runtime
                    .instantiate_wasm_module(&lowered_module)
                    .map_err(|error| error.to_string())?,
            );
        }
        Ok(lowered.as_ref().unwrap())
    }

    fn release_unreferenced(&mut self, previous: Option<InstanceRef>) {
        if let Some(previous) = previous.filter(|previous| Rc::strong_count(previous) == 1) {
            if let Instance::Ready { lowered, .. } = &mut *previous.borrow_mut() {
                if let Some(instance) = lowered.take() {
                    self.runtime.release_wasm(instance);
                }
            }
        }
    }

    fn host_reference(&mut self, id: u32) -> Option<quench_runtime::Value> {
        if let Some(&root) = self.host_references.get(&id) {
            return self.runtime.rooted_value(root);
        }
        // Script host tokens are opaque identities, never JS numeric values.
        let root = self.runtime.create_wasm_host_reference();
        self.host_references.insert(id, root);
        self.runtime.rooted_value(root)
    }

    fn argument(&mut self, arg: &wast::WastArg<'_>) -> Option<WasmValue> {
        match arg {
            wast::WastArg::Core(wast::core::WastArgCore::RefExtern(id)) => {
                Some(WasmValue::ExternRef(self.host_reference(*id)?))
            }
            wast::WastArg::Core(wast::core::WastArgCore::RefHost(id)) => {
                let value = self.host_reference(*id)?;
                self.runtime.internalize_wasm_reference(value).ok()
            }
            _ => shared_arg(arg),
        }
    }

    fn install(
        &mut self,
        name: Option<&str>,
        module: Result<Rc<Module>, String>,
    ) -> Result<(), String> {
        let prepared = module.and_then(|module| {
            self.prepare(module).map_err(|error| match error {
                LinkError::Exception => "uncaught Wasm exception".into(),
                LinkError::Trap(error)
                | LinkError::Unsupported(error)
                | LinkError::Unlinkable(error) => error,
            })
        });
        let error = prepared.as_ref().err().cloned();
        let instance = Rc::new(RefCell::new(prepared.unwrap_or_else(Instance::Unsupported)));
        let previous_named =
            name.and_then(|name| self.named.insert(name.to_owned(), instance.clone()));
        let previous_current = self.current.replace(instance);
        self.release_unreferenced(previous_current);
        self.release_unreferenced(previous_named);
        error.map_or(Ok(()), Err)
    }
}

impl Store {
    pub(crate) fn matches_result(&self, got: &WasmValue, want: &WastRetCore<'_>) -> bool {
        use wasmparser::AbstractHeapType as Heap;
        let kind = match want {
            WastRetCore::Either(options) => {
                return options.iter().any(|want| self.matches_result(got, want));
            }
            WastRetCore::RefHost(id) | WastRetCore::RefExtern(Some(id)) => {
                let expected = self
                    .host_references
                    .get(id)
                    .and_then(|&root| self.runtime.rooted_value(root));
                let actual = match (want, got) {
                    (WastRetCore::RefExtern(_), WasmValue::ExternRef(value)) => Some(*value),
                    (WastRetCore::RefHost(_), WasmValue::GcRef(_)) => {
                        self.runtime.externalize_wasm_reference(*got).ok()
                    }
                    _ => None,
                };
                return expected.is_some() && actual == expected;
            }
            WastRetCore::RefI31 => Heap::I31,
            WastRetCore::RefStruct => Heap::Struct,
            WastRetCore::RefArray => Heap::Array,
            WastRetCore::RefEq => Heap::Eq,
            WastRetCore::RefAny => Heap::Any,
            _ => return scalar_matches(want, *got),
        };
        self.runtime.wasm_value_matches_type(
            *got,
            WasmType::Reference {
                kind: quench_runtime::WasmReferenceKind::Internal(kind),
                nullable: false,
            },
        )
    }

    pub(crate) fn instantiate_quote(
        &mut self,
        module: &mut QuoteWat<'_>,
        _features: WasmFeatures,
    ) -> Result<(), String> {
        let name = module.name().map(|id| id.name().to_owned());
        let decoded = module
            .encode()
            .map(|bytes| Rc::new(Module { bytes }))
            .map_err(|error| error.to_string());
        self.install(name.as_deref(), decoded)
    }

    pub(crate) fn define_quote(&mut self, module: &mut QuoteWat<'_>, _features: WasmFeatures) {
        self.definitions.push(Definition {
            name: module.name().map(|id| id.name().to_owned()),
            module: module
                .encode()
                .map(|bytes| Rc::new(Module { bytes }))
                .map_err(|error| error.to_string()),
        });
    }

    pub(crate) fn instantiate_def(
        &mut self,
        instance: Option<&str>,
        module: Option<&str>,
        _features: WasmFeatures,
    ) -> Result<(), String> {
        let definition = match module {
            Some(name) => self
                .definitions
                .iter()
                .rev()
                .find(|d| d.name.as_deref() == Some(name)),
            None => self.definitions.last(),
        };
        let decoded = definition
            .map(|d| d.module.clone())
            .unwrap_or_else(|| Err("unknown module definition".into()));
        self.install(instance, decoded)
    }

    pub(crate) fn register(&mut self, name: &str, module: Option<&str>) -> Result<(), String> {
        let instance = match module {
            Some(name) => self.named.get(name),
            None => self.current.as_ref(),
        }
        .ok_or_else(|| "unknown module instance".to_owned())?;
        let result = match &*instance.borrow() {
            Instance::Ready { .. } => Ok(()),
            Instance::Unsupported(error) => Err(error.clone()),
        };
        let previous = self.named.insert(name.to_owned(), instance.clone());
        self.release_unreferenced(previous);
        result
    }

    pub(crate) fn run(
        &mut self,
        exec: &mut WastExecute<'_>,
        features: WasmFeatures,
    ) -> Outcome<WasmValue> {
        match exec {
            WastExecute::Invoke(invoke) => self.invoke(invoke),
            WastExecute::Get { module, global, .. } => {
                self.get_global(module.map(|id| id.name()), global)
            }
            WastExecute::Wat(wat) => {
                let bytes = match wat.encode() {
                    Ok(bytes) => bytes,
                    Err(error) => return Outcome::Unimplemented(error.to_string()),
                };
                match self.try_link(&bytes, features) {
                    Ok(()) => Outcome::Values(vec![]),
                    Err(LinkError::Exception) => Outcome::Exception,
                    Err(LinkError::Trap(error)) => Outcome::Trap(error.into()),
                    Err(LinkError::Unlinkable(error) | LinkError::Unsupported(error)) => {
                        Outcome::Unimplemented(error)
                    }
                }
            }
        }
    }

    pub(crate) fn invoke(&mut self, invoke: &WastInvoke<'_>) -> Outcome<WasmValue> {
        let args = match invoke
            .args
            .iter()
            .map(|arg| self.argument(arg))
            .collect::<Option<Vec<_>>>()
        {
            Some(args) => args,
            None => return Outcome::Unimplemented("unsupported shared Wasm argument type".into()),
        };
        let instance = match invoke.module {
            Some(id) => self.named.get(id.name()),
            None => self.current.as_ref(),
        };
        let Some(instance) = instance else {
            return Outcome::Missing;
        };
        let mut instance = instance.borrow_mut();
        let Instance::Ready {
            module,
            exports,
            lowered,
        } = &mut *instance
        else {
            let Instance::Unsupported(error) = &*instance else {
                unreachable!()
            };
            return Outcome::Unimplemented(error.clone());
        };
        let Some(&export) = exports.get(invoke.name) else {
            return Outcome::Unimplemented(format!("unknown function export: {}", invoke.name));
        };
        let result = match export {
            Export::Host(root) => self.runtime.invoke_wasm_host_function(root, &args),
            Export::Wasm(ExternalKind::Func, index) => {
                // One residual owns the module; exports are typed views of its entries.
                let instance = match Self::instantiate_bindings(&mut self.runtime, module, lowered)
                {
                    Ok(instance) => instance,
                    Err(error) => return Outcome::Unimplemented(error),
                };
                self.runtime.invoke_wasm_values(instance, index, &args)
            }
            _ => {
                return Outcome::Unimplemented(format!("unknown function export: {}", invoke.name));
            }
        };
        match result {
            Ok(values) => Outcome::Values(values),
            Err(error) if error.wasm_exception().is_some() => Outcome::Exception,
            Err(error) => match error.wasm_trap() {
                Some(_) => Outcome::Trap(error.to_string().into()),
                None => Outcome::Unimplemented(format!("shared execution failed: {error}")),
            },
        }
    }

    pub(crate) fn try_link(
        &mut self,
        bytes: &[u8],
        features: WasmFeatures,
    ) -> Result<(), LinkError> {
        match crate::inspect_binary_with(bytes, features) {
            crate::ModuleStatus::Valid => {}
            crate::ModuleStatus::ParseError(error) | crate::ModuleStatus::ValidateError(error) => {
                return Err(LinkError::Unsupported(format!(
                    "invalid instantiation input: {error}"
                )));
            }
        }
        let instance = self.prepare(Rc::new(Module {
            bytes: bytes.to_vec(),
        }))?;
        if let Instance::Ready {
            lowered, module, ..
        } = instance
        {
            let instance = match lowered {
                Some(instance) => instance,
                None => {
                    let definition = module
                        .lower_shared_module()
                        .map_err(|error| LinkError::Unsupported(error.to_string()))?;
                    self.instantiate_definition(&definition)?
                }
            };
            self.runtime.release_wasm(instance);
        }
        Ok(())
    }
}

fn scalar_arg(arg: &WastArg<'_>) -> Option<WasmValue> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Some(WasmValue::I32(*v)),
        WastArg::Core(WastArgCore::I64(v)) => Some(WasmValue::I64(*v)),
        WastArg::Core(WastArgCore::F32(v)) => Some(WasmValue::F32(v.bits)),
        WastArg::Core(WastArgCore::F64(v)) => Some(WasmValue::F64(v.bits)),
        _ => None,
    }
}

fn shared_arg(arg: &WastArg<'_>) -> Option<WasmValue> {
    use wast::core::{AbstractHeapType, HeapType};
    scalar_arg(arg).or_else(|| match arg {
        WastArg::Core(WastArgCore::V128(value)) => Some(WasmValue::V128(v128_bits(value))),
        WastArg::Core(WastArgCore::RefNull(HeapType::Abstract {
            shared: false,
            ty: AbstractHeapType::Func | AbstractHeapType::NoFunc,
        })) => Some(WasmValue::FuncRef(quench_runtime::Value::NULL)),
        WastArg::Core(WastArgCore::RefNull(HeapType::Abstract {
            shared: false,
            ty: AbstractHeapType::Extern | AbstractHeapType::NoExtern,
        })) => Some(WasmValue::ExternRef(quench_runtime::Value::NULL)),
        WastArg::Core(WastArgCore::RefNull(HeapType::Abstract {
            shared: false,
            ty:
                AbstractHeapType::Any
                | AbstractHeapType::Eq
                | AbstractHeapType::Struct
                | AbstractHeapType::Array
                | AbstractHeapType::I31
                | AbstractHeapType::None,
        })) => Some(WasmValue::GcRef(quench_runtime::Value::NULL)),
        _ => None,
    })
}

fn v128_bits(value: &wast::core::V128Const) -> u128 {
    u128::from_le_bytes(value.to_le_bytes())
}

fn v128_matches(pattern: &wast::core::V128Pattern, bits: u128) -> bool {
    let b = bits.to_le_bytes();
    match pattern {
        wast::core::V128Pattern::I8x16(vals) => {
            vals.iter().enumerate().all(|(i, lane)| *lane == b[i] as i8)
        }
        wast::core::V128Pattern::I16x8(vals) => vals
            .iter()
            .enumerate()
            .all(|(i, lane)| *lane == i16::from_le_bytes([b[i * 2], b[i * 2 + 1]])),
        wast::core::V128Pattern::I32x4(vals) => vals
            .iter()
            .enumerate()
            .all(|(i, lane)| *lane == i32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())),
        wast::core::V128Pattern::I64x2(vals) => vals
            .iter()
            .enumerate()
            .all(|(i, lane)| *lane == i64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap())),
        wast::core::V128Pattern::F32x4(vals) => vals.iter().enumerate().all(|(i, lane)| {
            let got = u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
            nan_f32(lane, got)
        }),
        wast::core::V128Pattern::F64x2(vals) => vals.iter().enumerate().all(|(i, lane)| {
            let got = u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
            nan_f64(lane, got)
        }),
    }
}

fn scalar_matches(want: &WastRetCore<'_>, got: WasmValue) -> bool {
    match (want, got) {
        (WastRetCore::Either(options), got) => options.iter().any(|v| scalar_matches(v, got)),
        (WastRetCore::V128(want), WasmValue::V128(got)) => v128_matches(want, got),
        (WastRetCore::I32(want), WasmValue::I32(got)) => *want == got,
        (WastRetCore::I64(want), WasmValue::I64(got)) => *want == got,
        (WastRetCore::F32(want), WasmValue::F32(got)) => nan_f32(want, got),
        (WastRetCore::F64(want), WasmValue::F64(got)) => nan_f64(want, got),
        (
            WastRetCore::RefNull(_),
            WasmValue::FuncRef(value) | WasmValue::ExternRef(value) | WasmValue::GcRef(value),
        ) => value.is_null(),
        (WastRetCore::RefExtern(None), WasmValue::ExternRef(value)) => !value.is_null(),
        (WastRetCore::RefExtern(Some(id)), WasmValue::ExternRef(value)) => {
            value.as_number() == Some(f64::from(*id))
        }
        (WastRetCore::RefFunc(None), WasmValue::FuncRef(value)) => !value.is_null(),
        _ => false,
    }
}

fn nan_f32(pattern: &wast::core::NanPattern<wast::token::F32>, bits: u32) -> bool {
    match pattern {
        wast::core::NanPattern::CanonicalNan => WasmValue::F32(bits).is_canonical_nan(),
        wast::core::NanPattern::ArithmeticNan => WasmValue::F32(bits).is_arithmetic_nan(),
        wast::core::NanPattern::Value(v) => v.bits == bits,
    }
}

fn nan_f64(pattern: &wast::core::NanPattern<wast::token::F64>, bits: u64) -> bool {
    match pattern {
        wast::core::NanPattern::CanonicalNan => WasmValue::F64(bits).is_canonical_nan(),
        wast::core::NanPattern::ArithmeticNan => WasmValue::F64(bits).is_arithmetic_nan(),
        wast::core::NanPattern::Value(v) => v.bits == bits,
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;

    #[test]
    fn gc_result_protocol_uses_live_kind_and_recurses_through_either() {
        let report = Engine::new().run_wast(
            "gc-result-protocol.wast",
            r#"
(module
  (type $s (struct))
  (type $a (array i32))
  (func (export "a") (result anyref) i32.const 0 array.new_default $a)
  (func (export "s") (result anyref) struct.new $s)
  (func (export "n") (result anyref) ref.null any)
  (func (export "i") (result anyref) i32.const 1 ref.i31))
(assert_return (invoke "s") (ref.struct))
(assert_return (invoke "s") (ref.eq))
(assert_return (invoke "s") (ref.any))
(assert_return (invoke "s") (either (ref.i31) (ref.struct)))
(assert_return (invoke "s") (ref.array))
(assert_return (invoke "s") (ref.i31))
(assert_return (invoke "s") (ref.null any))
(assert_return (invoke "n") (ref.struct))
(assert_return (invoke "i") (ref.struct))
(assert_return (invoke "a") (ref.struct))
(assert_return (invoke "a") (ref.i31))
(assert_return (invoke "a") (ref.array))
(assert_return (invoke "a") (either (ref.struct) (ref.array)))
"#,
        );
        assert_eq!(report.results.len(), 14);
        for (result, expected) in report.results.iter().zip([
            true, true, true, true, true, false, false, false, false, false, false, false, true,
            true,
        ]) {
            assert_eq!(result.passed, expected, "{result:?}");
        }
    }

    #[test]
    fn failed_module_replacement_is_not_a_fallback_to_previous_instance() {
        let report = Engine::new().run_wast(
            "lifecycle-control.wast",
            r#"
(module (func (export "f") (result i32) i32.const 17))
(module $failed (import "missing" "f" (func)) (func (export "f") (result i32) i32.const 17))
(assert_return (invoke "f") (i32.const 17))
(register "failed-alias" $failed)
(assert_return (invoke $failed "f") (i32.const 17))
"#,
        );
        assert_eq!(report.results.len(), 5);
        assert!(report.results[0].passed);
        assert!(report.results[1]
            .got
            .contains("unknown import module: missing"));
        for result in &report.results[1..] {
            assert!(
                !result.passed,
                "failed replacement reused a previous instance: {result:?}"
            );
        }
    }
}
