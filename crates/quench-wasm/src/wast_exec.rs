//! Execute the scalar subset of WAST through the shared Quench VM.

use std::collections::HashMap;

use rqj::WasmSignature;
use rqj::{
    Runtime, SystemHost, WasmFunctionRef, WasmGlobalId, WasmMemoryId, WasmModuleId, WasmTableId,
    WasmValue,
};
use wasmparser::{Parser, Payload, WasmFeatures};
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::{QuoteWat, WastArg, WastExecute, WastInvoke, WastRet, Wat};

use crate::{Engine, wast_script::DirectiveResult};

#[derive(Clone)]
struct Instance {
    module: Option<WasmModuleId>,
    exports: HashMap<String, u32>,
    function_exports: HashMap<String, WasmSignature>,
    function_export_fingerprints: HashMap<String, String>,
    export_names: std::collections::HashSet<String>,
    tag_exports: HashMap<String, WasmSignature>,
    tag_export_fingerprints: HashMap<String, String>,
    tag_export_indices: HashMap<String, u32>,
    global_exports: HashMap<String, (u32, rqj::WasmType, bool)>,
    global_export_fingerprints: HashMap<String, String>,
    table_exports: HashMap<String, u32>,
    table_export_types: HashMap<String, crate::shared::TableImport>,
    memory_exports: HashMap<String, u32>,
    unsupported_reason: Option<String>,
    instantiation_trap: Option<String>,
}

pub struct Store {
    runtime: Runtime<SystemHost>,
    current: Option<Instance>,
    named: HashMap<String, Instance>,
    defs: HashMap<String, Vec<u8>>,
    spectest_memory: Option<WasmMemoryId>,
    spectest_shared_memory: Option<WasmMemoryId>,
    spectest_table: Option<WasmTableId>,
}

impl Store {
    pub fn new() -> Self {
        let mut store = Self {
            runtime: Runtime::new(SystemHost),
            current: None,
            named: HashMap::new(),
            defs: HashMap::new(),
            spectest_memory: None,
            spectest_shared_memory: None,
            spectest_table: None,
        };
        store.install_spectest_resources();
        store
    }

    fn install_spectest_resources(&mut self) {
        let module = Engine::new()
            .compile_wat_with_features(
                r#"(module
                    (memory (export "memory") 1 2)
                    (memory (export "shared_memory") 1 2 shared)
                    (table (export "table") 10 20 funcref)
                    (global (export "global_i32") i32 (i32.const 666))
                    (global (export "global_i64") i64 (i64.const 666))
                    (global (export "global_f32") f32 (f32.const 666.6))
                    (global (export "global_f64") f64 (f64.const 666.6))
                    (func (export "print"))
                    (func (export "print_i32") (param i32))
                    (func (export "print_i64") (param i64))
                    (func (export "print_f32") (param f32))
                    (func (export "print_f64") (param f64))
                    (func (export "print_i32_f32") (param i32 f32))
                    (func (export "print_f64_f64") (param f64 f64))
                )"#,
                WasmFeatures::default() | WasmFeatures::THREADS,
            )
            .expect("the fixed WAST spectest resources must validate");
        let instance = self.build(
            module.bytes(),
            WasmFeatures::default() | WasmFeatures::THREADS,
        );
        assert!(
            instance.unsupported_reason.is_none(),
            "the fixed WAST spectest module must instantiate"
        );
        let module_id = instance.module.expect("spectest module is installed");
        self.spectest_memory = instance
            .memory_exports
            .get("memory")
            .and_then(|index| self.runtime.wasm_memory_id(module_id, *index));
        self.spectest_shared_memory = instance
            .memory_exports
            .get("shared_memory")
            .and_then(|index| self.runtime.wasm_memory_id(module_id, *index));
        self.spectest_table = instance
            .table_exports
            .get("table")
            .and_then(|index| self.runtime.wasm_table_id(module_id, *index));
        self.named.insert("spectest".into(), instance);
    }

    pub fn instantiate_quote(&mut self, quote: &mut QuoteWat<'_>, features: WasmFeatures) {
        let name = quote.name().map(|id| id.name().to_string());
        let instance = match quote.encode() {
            Ok(bytes) => self.build(&bytes, features),
            Err(error) => unsupported_instance(HashMap::new(), error.to_string()),
        };
        if let Some(name) = name {
            self.named.insert(name, instance.clone());
        }
        self.current = Some(instance);
    }

    pub fn define_quote(&mut self, quote: &mut QuoteWat<'_>, _: WasmFeatures) {
        if let (Some(name), Ok(bytes)) =
            (quote.name().map(|id| id.name().to_string()), quote.encode())
        {
            self.defs.insert(name, bytes);
        }
    }

    pub fn instantiate_def(
        &mut self,
        instance: Option<&str>,
        module: Option<&str>,
        features: WasmFeatures,
    ) {
        let Some(bytes) = module.and_then(|name| self.defs.get(name)).cloned() else {
            self.current = Some(unsupported_instance(
                HashMap::new(),
                "unknown module definition".into(),
            ));
            return;
        };
        let built = self.build(&bytes, features);
        if let Some(name) = instance {
            self.named.insert(name.to_string(), built.clone());
        }
        self.current = Some(built);
    }

    pub fn register(&mut self, name: &str, module: Option<&str>) {
        let instance = module
            .and_then(|name| self.named.get(name))
            .or_else(|| module.is_none().then(|| self.current.as_ref()).flatten());
        if let Some(instance) = instance {
            self.named.insert(name.to_string(), instance.clone());
        }
    }

    fn build(&mut self, bytes: &[u8], features: WasmFeatures) -> Instance {
        let module = match checked_module(bytes, features) {
            Ok(module) => module,
            Err(error) => return unsupported_instance(HashMap::new(), error.to_string()),
        };
        let (function_exports, function_imports, function_export_fingerprints, _) =
            match module.function_link_metadata() {
                Ok(result) => result,
                Err(error) => return unsupported_instance(HashMap::new(), error.to_string()),
            };
        let export_names = match module.export_names() {
            Ok(names) => names,
            Err(error) => return unsupported_instance(function_exports, error.to_string()),
        };
        let (tag_exports, tag_imports, tag_export_fingerprints, _) =
            match module.tag_link_metadata() {
                Ok(metadata) => metadata,
                Err(error) => return unsupported_instance(function_exports, error.to_string()),
            };
        let (_, tag_export_indices) = match module.tag_link_indices() {
            Ok(metadata) => metadata,
            Err(error) => return unsupported_instance(function_exports, error.to_string()),
        };
        let (global_imports, global_export_fingerprints) = match module.global_link_metadata() {
            Ok(metadata) => metadata,
            Err(error) => return unsupported_instance(function_exports, error.to_string()),
        };
        let (table_imports, table_exports, table_export_types) = match module.table_link_metadata()
        {
            Ok(result) => result,
            Err(error) => return unsupported_instance(function_exports, error.to_string()),
        };
        let (memory_imports, memory_exports) = match module.memory_link_metadata() {
            Ok(result) => result,
            Err(error) => return unsupported_instance(function_exports, error.to_string()),
        };
        let Some(imported_functions) = function_imports
            .iter()
            .map(|(module_name, name, signature)| {
                self.resolve_function_import(module_name, name, signature)
                    .map(Some)
            })
            .collect::<Option<Vec<_>>>()
        else {
            let missing = function_imports.iter().find(|(module, name, signature)| {
                self.resolve_function_import(module, name, signature)
                    .is_none()
            });
            let reason = missing.map_or_else(
                || "unresolved or incompatible function import".to_string(),
                |(module, name, signature)| {
                    format!(
                        "unresolved or incompatible function import {module}.{name} {signature:?}"
                    )
                },
            );
            return unsupported_instance(function_exports, reason);
        };
        let Some(imported_tables) = table_imports
            .iter()
            .map(|import| self.resolve_table_import(import))
            .collect::<Option<Vec<_>>>()
        else {
            return unsupported_instance(
                function_exports,
                "unresolved or incompatible table import".into(),
            );
        };
        let Some(imported_memories) = memory_imports
            .iter()
            .map(|import| self.resolve_memory_import(import))
            .collect::<Option<Vec<_>>>()
        else {
            return unsupported_instance(
                function_exports,
                "unresolved or incompatible memory import".into(),
            );
        };
        let Some(imported_tags) = tag_imports
            .iter()
            .map(|(module_name, name, signature)| {
                self.resolve_tag_import(module_name, name, signature)
                    .map(Some)
            })
            .collect::<Option<Vec<_>>>()
        else {
            return unsupported_instance(
                function_exports,
                "unresolved or incompatible exception tag import".into(),
            );
        };
        let imported_globals = global_imports
            .iter()
            .map(|import| self.resolve_global_import(import))
            .collect::<Option<Vec<_>>>();
        let Some(imported_globals) = imported_globals else {
            return unsupported_instance(
                function_exports,
                "unresolved or incompatible global import".into(),
            );
        };
        let imported_values = imported_globals
            .iter()
            .map(|(value, mutable, _)| (*value, *mutable))
            .collect::<Vec<_>>();
        let imported_ids = imported_globals
            .iter()
            .map(|(_, _, global)| *global)
            .collect::<Vec<_>>();
        let (lowered, exports, global_exports) = match module.lower_shared_module_with_imports(
            &imported_values,
            &imported_functions,
            &imported_tables,
            &imported_memories,
            &imported_tags,
        ) {
            Ok(result) => result,
            Err(error) => {
                let mut instance = unsupported_instance(function_exports, error.to_string());
                instance.table_exports = table_exports;
                instance.table_export_types = table_export_types;
                instance.memory_exports = memory_exports;
                return instance;
            }
        };
        match self.runtime.install_wasm_module_with_full_imports(
            &lowered,
            &imported_ids,
            &imported_tables,
            &imported_memories,
        ) {
            Ok(module) => Instance {
                module: Some(module),
                exports,
                function_exports,
                function_export_fingerprints,
                export_names,
                tag_exports,
                tag_export_fingerprints,
                tag_export_indices,
                global_exports,
                global_export_fingerprints,
                table_exports,
                table_export_types,
                memory_exports,
                unsupported_reason: None,
                instantiation_trap: None,
            },
            Err(error) => Instance {
                module: None,
                exports: HashMap::new(),
                function_exports,
                function_export_fingerprints,
                export_names,
                tag_exports,
                tag_export_fingerprints,
                tag_export_indices,
                global_exports: HashMap::new(),
                global_export_fingerprints,
                table_exports,
                table_export_types,
                memory_exports,
                unsupported_reason: None,
                instantiation_trap: Some(error.to_string()),
            },
        }
    }

    fn resolve_global_import(
        &self,
        import: &crate::shared::GlobalImport,
    ) -> Option<(WasmValue, bool, Option<WasmGlobalId>)> {
        let (value, mutable, identity) = if import.module == "spectest" {
            let value = match import.name.as_str() {
                "global_i32" => WasmValue::I32(666),
                "global_i64" => WasmValue::I64(666),
                "global_f32" => WasmValue::F32(666.6f32.to_bits()),
                "global_f64" => WasmValue::F64(666.6f64.to_bits()),
                _ => return None,
            };
            (value, false, None)
        } else {
            let instance = self.named.get(&import.module)?;
            let module = instance.module?;
            let (index, ty, mutable) = instance.global_exports.get(&import.name).copied()?;
            if ty != import.ty
                || instance
                    .global_export_fingerprints
                    .get(&import.name)
                    .is_some_and(|fingerprint| fingerprint != &import.fingerprint)
            {
                return None;
            }
            let identity = self.runtime.wasm_global_id(module, index)?;
            (
                self.runtime.wasm_global(module, index)?,
                mutable,
                Some(identity),
            )
        };
        if value.ty() != import.ty || mutable != import.mutable {
            return None;
        }
        Some((value, mutable, identity))
    }

    fn resolve_function_import(
        &self,
        module_name: &str,
        name: &str,
        signature: &WasmSignature,
    ) -> Option<WasmFunctionRef> {
        let instance = self.named.get(module_name)?;
        if instance.function_exports.get(name)? != signature {
            return None;
        }
        Some(WasmFunctionRef {
            module: instance.module?,
            function_index: *instance.exports.get(name)?,
        })
    }

    fn resolve_tag_import(
        &self,
        module_name: &str,
        name: &str,
        signature: &WasmSignature,
    ) -> Option<rqj::WasmTagId> {
        let instance = self.named.get(module_name)?;
        if instance.tag_exports.get(name)? != signature {
            return None;
        }
        let module = instance.module?;
        self.runtime
            .wasm_tag_id(module, *instance.tag_export_indices.get(name)?)
    }

    fn resolve_table_import(
        &self,
        import: &crate::shared::TableImport,
    ) -> Option<Option<WasmTableId>> {
        if import.module == "spectest" {
            let actual = self
                .named
                .get("spectest")?
                .table_export_types
                .get(&import.name)?;
            if actual.element_fingerprint != import.element_fingerprint {
                return None;
            }
            let table = (import.name == "table")
                .then_some(self.spectest_table)
                .flatten()?;
            let (element_type, table64, size, maximum) = self.runtime.wasm_table_info(table)?;
            return (table64 == import.table64
                && element_type == import.element_type
                && size >= import.initial_size
                && import
                    .maximum_size
                    .is_none_or(|expected| maximum.is_some_and(|actual| actual <= expected)))
            .then_some(Some(table));
        }
        let instance = self.named.get(&import.module)?;
        let actual = instance.table_export_types.get(&import.name)?;
        if actual.element_fingerprint != import.element_fingerprint {
            return None;
        }
        let module = instance.module?;
        let table_index = *instance.table_exports.get(&import.name)?;
        let table = self.runtime.wasm_table_id(module, table_index)?;
        let (element_type, table64, size, maximum) = self.runtime.wasm_table_info(table)?;
        if table64 != import.table64
            || element_type != import.element_type
            || size < import.initial_size
            || import
                .maximum_size
                .is_some_and(|expected| maximum.is_none_or(|actual| actual > expected))
        {
            return None;
        }
        Some(Some(table))
    }

    fn resolve_memory_import(
        &self,
        import: &crate::shared::MemoryImport,
    ) -> Option<Option<WasmMemoryId>> {
        if import.module == "spectest" {
            let memory = match import.name.as_str() {
                "memory" => self.spectest_memory,
                "shared_memory" => self.spectest_shared_memory,
                _ => None,
            }?;
            let (memory64, size, maximum, page_size_log2, shared) =
                self.runtime.wasm_memory_info(memory)?;
            return (memory64 == import.memory64
                && shared == import.shared
                && page_size_log2 == import.page_size_log2
                && size >= import.initial_pages
                && import
                    .maximum_pages
                    .is_none_or(|expected| maximum.is_some_and(|actual| actual <= expected)))
            .then_some(Some(memory));
        }
        let instance = self.named.get(&import.module)?;
        let module = instance.module?;
        let memory_index = *instance.memory_exports.get(&import.name)?;
        let memory = self.runtime.wasm_memory_id(module, memory_index)?;
        let (memory64, size, maximum, page_size_log2, shared) =
            self.runtime.wasm_memory_info(memory)?;
        if memory64 != import.memory64
            || shared != import.shared
            || page_size_log2 != import.page_size_log2
            || size < import.initial_pages
            || import
                .maximum_pages
                .is_some_and(|expected| maximum.is_none_or(|actual| actual > expected))
        {
            return None;
        }
        Some(Some(memory))
    }

    fn instance(&self, module: Option<wast::token::Id<'_>>) -> Option<&Instance> {
        module
            .and_then(|id| self.named.get(id.name()))
            .or_else(|| module.is_none().then_some(self.current.as_ref()).flatten())
    }
}

fn unsupported_instance(
    function_exports: HashMap<String, WasmSignature>,
    reason: String,
) -> Instance {
    Instance {
        module: None,
        exports: HashMap::new(),
        function_exports,
        function_export_fingerprints: HashMap::new(),
        export_names: std::collections::HashSet::new(),
        tag_exports: HashMap::new(),
        tag_export_fingerprints: HashMap::new(),
        tag_export_indices: HashMap::new(),
        global_exports: HashMap::new(),
        global_export_fingerprints: HashMap::new(),
        table_exports: HashMap::new(),
        table_export_types: HashMap::new(),
        memory_exports: HashMap::new(),
        unsupported_reason: Some(reason),
        instantiation_trap: None,
    }
}

fn checked_module(bytes: &[u8], features: WasmFeatures) -> Result<crate::Module, crate::Error> {
    match crate::inspect_binary_with(bytes, features) {
        crate::ModuleStatus::Valid => Ok(crate::Module {
            bytes: bytes.to_vec(),
        }),
        crate::ModuleStatus::ParseError(message) => Err(crate::Error::Parse(message)),
        crate::ModuleStatus::ValidateError(message) => Err(crate::Error::Validate(message)),
    }
}

enum Outcome {
    Values(Vec<WasmValue>),
    Trap(String),
    Exception(String),
    Unimplemented(String),
    Missing,
}

pub fn score_return(
    line: usize,
    exec: &mut WastExecute<'_>,
    expected: &[WastRet<'_>],
    store: &mut Store,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_return".to_string();
    match run(exec, store, features) {
        Outcome::Unimplemented(reason) => unimplemented(line, &kind, &reason),
        Outcome::Values(got) => compare_rets(line, kind, expected, &got),
        Outcome::Trap(got) => fail(line, kind, "return", &got),
        Outcome::Exception(got) => fail(line, kind, "return", &got),
        Outcome::Missing => fail(line, kind, "return", "unknown export"),
    }
}

pub fn score_trap(
    line: usize,
    exec: &mut WastExecute<'_>,
    message: &str,
    store: &mut Store,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_trap".to_string();
    let expected = format!("trap ({message})");
    match run(exec, store, features) {
        Outcome::Unimplemented(reason) => unimplemented(line, &kind, &reason),
        Outcome::Trap(got) if trap_matches(&got, message) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got,
        },
        Outcome::Trap(got) => fail(line, kind, &expected, &got),
        Outcome::Exception(got) => fail(line, kind, &expected, &got),
        Outcome::Values(_) => fail(line, kind, &expected, "return"),
        Outcome::Missing => fail(line, kind, &expected, "unknown export"),
    }
}

pub fn score_exception(
    line: usize,
    exec: &mut WastExecute<'_>,
    store: &mut Store,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_exception".to_string();
    match run(exec, store, features) {
        Outcome::Unimplemented(reason) => unimplemented(line, &kind, &reason),
        Outcome::Exception(got) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected: "exception".into(),
            got,
        },
        Outcome::Trap(got) => fail(line, kind, "exception", &got),
        Outcome::Values(_) => fail(line, kind, "exception", "return"),
        Outcome::Missing => fail(line, kind, "exception", "unknown export"),
    }
}

pub fn score_unlinkable(
    line: usize,
    module: &mut Wat<'_>,
    message: &str,
    features: WasmFeatures,
    store: &Store,
) -> DirectiveResult {
    let kind = "assert_unlinkable".to_string();
    let expected = format!("unlinkable ({message})");
    let bytes = match module.encode() {
        Ok(bytes) => bytes,
        Err(error) => {
            return DirectiveResult {
                line,
                kind,
                passed: true,
                expected,
                got: error.to_string(),
            };
        }
    };
    let module = match checked_module(&bytes, features) {
        Ok(module) => module,
        Err(error) => return fail(line, kind, &expected, &error.to_string()),
    };
    let Ok((_, function_imports, _, function_import_fingerprints)) =
        module.function_link_metadata()
    else {
        return unimplemented(line, &kind, "unsupported import metadata");
    };
    let Ok((table_imports, _, _)) = module.table_link_metadata() else {
        return unimplemented(line, &kind, "unsupported table import metadata");
    };
    let Ok((memory_imports, _)) = module.memory_link_metadata() else {
        return unimplemented(line, &kind, "unsupported memory import metadata");
    };
    let Ok((global_imports, _)) = module.global_link_metadata() else {
        return unimplemented(line, &kind, "unsupported global import metadata");
    };
    let Ok((_, tag_imports, _, tag_import_fingerprints)) = module.tag_link_metadata() else {
        return unimplemented(line, &kind, "unsupported tag import metadata");
    };
    let modules = import_modules(&bytes);
    if modules.is_empty() {
        return fail(line, kind, &expected, "linked");
    }
    for import_module in modules {
        if import_module != "spectest" && !store.named.contains_key(&import_module) {
            return unlinkable(line, kind, expected, message, "unknown import");
        }
    }
    for (import_module, name, wanted) in function_imports {
        let Some(instance) = store.named.get(&import_module) else {
            continue;
        };
        let Some(actual) = instance.function_exports.get(&name) else {
            let reason = if instance_has_export(instance, &name) {
                "incompatible import type"
            } else {
                "unknown import"
            };
            return unlinkable(line, kind, expected, message, reason);
        };
        if actual.params != wanted.params
            || actual.result != wanted.result
            || actual.additional_results != wanted.additional_results
        {
            return unlinkable(line, kind, expected, message, "incompatible import type");
        }
    }
    for (import_module, name, wanted) in tag_imports {
        let Some(instance) = store.named.get(&import_module) else {
            continue;
        };
        let Some(actual) = instance.tag_exports.get(&name) else {
            let reason = if instance.export_names.contains(&name) {
                "incompatible import type"
            } else {
                "unknown import"
            };
            return unlinkable(line, kind, expected, message, reason);
        };
        if actual != &wanted {
            return unlinkable(line, kind, expected, message, "incompatible import type");
        }
    }
    for (import_module, name, wanted) in tag_import_fingerprints {
        let Some(instance) = store.named.get(&import_module) else {
            continue;
        };
        if instance
            .tag_export_fingerprints
            .get(&name)
            .is_some_and(|actual| actual != &wanted)
        {
            return unlinkable(line, kind, expected, message, "incompatible import type");
        }
    }
    for (import_module, name, wanted) in function_import_fingerprints {
        let Some(instance) = store.named.get(&import_module) else {
            continue;
        };
        if instance
            .function_export_fingerprints
            .get(&name)
            .is_some_and(|actual| actual != &wanted)
        {
            return unlinkable(line, kind, expected, message, "incompatible import type");
        }
    }
    for import in table_imports {
        let Some(instance) = store.named.get(&import.module) else {
            continue;
        };
        let Some(actual) = instance.table_export_types.get(&import.name) else {
            let reason = if instance_has_export(instance, &import.name) {
                "incompatible import type"
            } else {
                "unknown import"
            };
            return unlinkable(line, kind, expected, message, reason);
        };
        if actual.table64 != import.table64
            || actual.element_type != import.element_type
            || actual.element_fingerprint != import.element_fingerprint
            || actual.initial_size < import.initial_size
            || import
                .maximum_size
                .is_some_and(|wanted| actual.maximum_size.is_none_or(|actual| actual > wanted))
        {
            return unlinkable(line, kind, expected, message, "incompatible import type");
        }
    }
    for import in memory_imports {
        let Some(instance) = store.named.get(&import.module) else {
            continue;
        };
        if !instance.memory_exports.contains_key(&import.name) {
            let reason = if instance_has_export(instance, &import.name) {
                "incompatible import type"
            } else {
                "unknown import"
            };
            return unlinkable(line, kind, expected, message, reason);
        }
        if store.resolve_memory_import(&import).is_none() {
            return unlinkable(line, kind, expected, message, "incompatible import type");
        }
    }
    for import in global_imports {
        let Some(instance) = store.named.get(&import.module) else {
            continue;
        };
        let Some((_, actual_type, actual_mutable)) = instance.global_exports.get(&import.name)
        else {
            let reason = if instance_has_export(instance, &import.name) {
                "incompatible import type"
            } else {
                "unknown import"
            };
            return unlinkable(line, kind, expected, message, reason);
        };
        if *actual_type != import.ty || *actual_mutable != import.mutable {
            return unlinkable(line, kind, expected, message, "incompatible import type");
        }
        if instance
            .global_export_fingerprints
            .get(&import.name)
            .is_some_and(|actual| actual != &import.fingerprint)
        {
            return unlinkable(line, kind, expected, message, "incompatible import type");
        }
    }
    fail(line, kind, &expected, "linked")
}

fn unlinkable(
    line: usize,
    kind: String,
    expected: String,
    message: &str,
    got: &str,
) -> DirectiveResult {
    if trap_matches(got, message) {
        DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got: got.into(),
        }
    } else {
        fail(line, kind, &expected, got)
    }
}

fn instance_has_export(instance: &Instance, name: &str) -> bool {
    instance.export_names.contains(name)
        || instance.function_exports.contains_key(name)
        || instance.global_exports.contains_key(name)
        || instance.table_export_types.contains_key(name)
        || instance.memory_exports.contains_key(name)
        || instance.tag_exports.contains_key(name)
}

pub fn score_exhaustion(
    line: usize,
    invoke: &WastInvoke<'_>,
    message: &str,
    store: &mut Store,
) -> DirectiveResult {
    let kind = "assert_exhaustion".to_string();
    let expected = format!("exhaustion ({message})");
    match run_invoke(invoke, store) {
        Outcome::Unimplemented(reason) => unimplemented(line, &kind, &reason),
        Outcome::Trap(got) if got.contains("exhaust") => DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got,
        },
        Outcome::Trap(got) => fail(line, kind, &expected, &got),
        Outcome::Exception(got) => fail(line, kind, &expected, &got),
        Outcome::Values(_) => fail(line, kind, &expected, "return"),
        Outcome::Missing => fail(line, kind, &expected, "unknown export"),
    }
}

pub fn score_invoke(line: usize, invoke: &WastInvoke<'_>, store: &mut Store) -> DirectiveResult {
    let kind = "invoke".to_string();
    match run_invoke(invoke, store) {
        Outcome::Unimplemented(reason) => unimplemented(line, &kind, &reason),
        Outcome::Values(_) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected: "invoke".into(),
            got: "ok".into(),
        },
        Outcome::Trap(got) => fail(line, kind, "invoke", &got),
        Outcome::Exception(got) => fail(line, kind, "invoke", &got),
        Outcome::Missing => fail(line, kind, "invoke", "unknown export"),
    }
}

fn run(exec: &mut WastExecute<'_>, store: &mut Store, features: WasmFeatures) -> Outcome {
    match exec {
        WastExecute::Invoke(invoke) => run_invoke(invoke, store),
        WastExecute::Get { module, global, .. } => run_get(*module, global, store),
        WastExecute::Wat(wat) => run_wat(wat, store, features),
    }
}

fn run_get(module: Option<wast::token::Id<'_>>, global: &str, store: &Store) -> Outcome {
    let Some(instance) = store.instance(module) else {
        return Outcome::Missing;
    };
    let Some((index, _, _)) = instance.global_exports.get(global) else {
        return Outcome::Missing;
    };
    let Some(module) = instance.module else {
        return Outcome::Unimplemented(
            instance
                .unsupported_reason
                .clone()
                .unwrap_or_else(|| "shared module is unavailable".into()),
        );
    };
    store
        .runtime
        .wasm_global(module, *index)
        .map(|value| Outcome::Values(vec![value]))
        .unwrap_or(Outcome::Missing)
}

fn run_wat(wat: &mut Wat<'_>, store: &mut Store, features: WasmFeatures) -> Outcome {
    let bytes = match wat.encode() {
        Ok(bytes) => bytes,
        Err(error) => return Outcome::Trap(error.to_string()),
    };
    let module = match checked_module(&bytes, features) {
        Ok(module) => module,
        Err(error) => return Outcome::Unimplemented(error.to_string()),
    };
    let instance = store.build(module.bytes(), features);
    if let Some(trap) = instance.instantiation_trap {
        Outcome::Trap(trap)
    } else if let Some(reason) = instance.unsupported_reason {
        Outcome::Unimplemented(reason)
    } else if instance.module.is_some() {
        Outcome::Values(Vec::new())
    } else {
        Outcome::Unimplemented("shared module is unavailable".into())
    }
}

fn run_invoke(invoke: &WastInvoke<'_>, store: &mut Store) -> Outcome {
    let Some(instance) = store.instance(invoke.module).cloned() else {
        return Outcome::Missing;
    };
    let Some(module) = instance.module else {
        return Outcome::Unimplemented(
            instance
                .unsupported_reason
                .unwrap_or_else(|| "shared module is unavailable".into()),
        );
    };
    let Some(function) = instance.exports.get(invoke.name).copied() else {
        return Outcome::Missing;
    };
    let Some(args) = args_values(&invoke.args) else {
        return Outcome::Unimplemented("unsupported WAST argument type".into());
    };
    match store.runtime.invoke_wasm_function(module, function, &args) {
        Ok(values) => Outcome::Values(values),
        Err(error) if error.wasm_exception().is_some() => Outcome::Exception(error.to_string()),
        Err(error) => Outcome::Trap(error.to_string()),
    }
}

fn args_values(args: &[WastArg<'_>]) -> Option<Vec<WasmValue>> {
    args.iter()
        .map(|arg| match arg {
            WastArg::Core(WastArgCore::I32(value)) => Some(WasmValue::I32(*value)),
            WastArg::Core(WastArgCore::I64(value)) => Some(WasmValue::I64(*value)),
            WastArg::Core(WastArgCore::F32(value)) => Some(WasmValue::F32(value.bits)),
            WastArg::Core(WastArgCore::F64(value)) => Some(WasmValue::F64(value.bits)),
            WastArg::Core(WastArgCore::V128(value)) => {
                Some(WasmValue::V128(u128::from_le_bytes(value.to_le_bytes())))
            }
            WastArg::Core(WastArgCore::RefNull(heap_type)) => Some(match heap_type {
                wast::core::HeapType::Abstract {
                    ty: wast::core::AbstractHeapType::Func | wast::core::AbstractHeapType::NoFunc,
                    ..
                }
                | wast::core::HeapType::Concrete(_) => WasmValue::FuncRef(None),
                wast::core::HeapType::Abstract {
                    ty: wast::core::AbstractHeapType::I31,
                    ..
                } => WasmValue::I31Ref(None),
                _ => WasmValue::ExternRef(None),
            }),
            WastArg::Core(WastArgCore::RefExtern(index) | WastArgCore::RefHost(index)) => {
                Some(WasmValue::ExternRef(Some(*index)))
            }
            _ => None,
        })
        .collect()
}

fn compare_rets(
    line: usize,
    kind: String,
    expected: &[WastRet<'_>],
    got: &[WasmValue],
) -> DirectiveResult {
    if expected.len() != got.len() {
        return fail(
            line,
            kind,
            &format!("{} values", expected.len()),
            &format!("{} values", got.len()),
        );
    }
    for (want, got) in expected.iter().zip(got) {
        if !ret_matches(want, *got) {
            return fail(line, kind, &format!("{want:?}"), &format!("{got:?}"));
        }
    }
    DirectiveResult {
        line,
        kind,
        passed: true,
        expected: "match".into(),
        got: "match".into(),
    }
}

fn ret_matches(want: &WastRet<'_>, got: WasmValue) -> bool {
    if let WastRet::Core(WastRetCore::Either(options)) = want {
        return options.iter().any(|option| core_ret_matches(option, got));
    }
    match want {
        WastRet::Core(value) => core_ret_matches(value, got),
        _ => false,
    }
}

fn core_ret_matches(want: &WastRetCore, got: WasmValue) -> bool {
    match (want, got) {
        (WastRetCore::I32(want), WasmValue::I32(got)) => *want == got,
        (WastRetCore::I64(want), WasmValue::I64(got)) => *want == got,
        (WastRetCore::F32(pattern), WasmValue::F32(bits)) => nan_f32(pattern, bits),
        (WastRetCore::F64(pattern), WasmValue::F64(bits)) => nan_f64(pattern, bits),
        (WastRetCore::V128(pattern), WasmValue::V128(bits)) => v128_matches(pattern, bits),
        (WastRetCore::RefI31, WasmValue::I31Ref(Some(_))) => true,
        (WastRetCore::RefNull(heap_type), value) => ref_null_matches(*heap_type, value),
        (WastRetCore::RefExtern(Some(want)), WasmValue::ExternRef(Some(got))) => *want == got,
        (WastRetCore::RefHost(want), WasmValue::ExternRef(Some(got))) => *want == got,
        (WastRetCore::RefExtern(None), WasmValue::ExternRef(Some(_))) => true,
        (
            WastRetCore::RefAny | WastRetCore::RefEq,
            WasmValue::FuncRef(Some(_)) | WasmValue::ExternRef(Some(_)),
        ) => true,
        (WastRetCore::RefArray, WasmValue::FuncRef(Some(reference))) => {
            reference >> rqj::WASM_GC_REFERENCE_CLASS_SHIFT == rqj::WASM_GC_REFERENCE_ARRAY_CLASS
        }
        (WastRetCore::RefStruct, WasmValue::FuncRef(Some(reference))) => {
            reference >> rqj::WASM_GC_REFERENCE_CLASS_SHIFT == rqj::WASM_GC_REFERENCE_STRUCT_CLASS
        }
        (WastRetCore::RefFunc(None), WasmValue::FuncRef(Some(reference))) => {
            reference >> rqj::WASM_GC_REFERENCE_CLASS_SHIFT == 0
        }
        (
            WastRetCore::RefFunc(Some(wast::token::Index::Num(want, _))),
            WasmValue::FuncRef(Some(got)),
        ) => *want == got,
        _ => false,
    }
}

fn v128_matches(pattern: &wast::core::V128Pattern, bits: u128) -> bool {
    use wast::core::V128Pattern;
    match pattern {
        V128Pattern::I8x16(values) => {
            (0..16).all(|index| ((bits >> (index * 8)) as u8 as i8) == values[index])
        }
        V128Pattern::I16x8(values) => {
            (0..8).all(|index| ((bits >> (index * 16)) as u16 as i16) == values[index])
        }
        V128Pattern::I32x4(values) => {
            (0..4).all(|index| ((bits >> (index * 32)) as u32 as i32) == values[index])
        }
        V128Pattern::I64x2(values) => {
            (0..2).all(|index| ((bits >> (index * 64)) as u64 as i64) == values[index])
        }
        V128Pattern::F32x4(values) => {
            (0..4).all(|index| nan_f32(&values[index], (bits >> (index * 32)) as u32))
        }
        V128Pattern::F64x2(values) => {
            (0..2).all(|index| nan_f64(&values[index], (bits >> (index * 64)) as u64))
        }
    }
}

fn ref_null_matches(heap_type: Option<wast::core::HeapType<'_>>, value: WasmValue) -> bool {
    match (heap_type, value) {
        (
            None,
            WasmValue::FuncRef(None)
            | WasmValue::ExternRef(None)
            | WasmValue::I31Ref(None)
            | WasmValue::ExnRef(None),
        ) => true,
        (
            Some(wast::core::HeapType::Abstract {
                ty: wast::core::AbstractHeapType::Func | wast::core::AbstractHeapType::NoFunc,
                ..
            }),
            WasmValue::FuncRef(None),
        ) => true,
        (
            Some(wast::core::HeapType::Abstract {
                ty: wast::core::AbstractHeapType::Extern | wast::core::AbstractHeapType::NoExtern,
                ..
            }),
            WasmValue::ExternRef(None),
        ) => true,
        (
            Some(wast::core::HeapType::Abstract {
                ty: wast::core::AbstractHeapType::I31,
                ..
            }),
            WasmValue::I31Ref(None),
        ) => true,
        (
            Some(wast::core::HeapType::Abstract {
                ty: wast::core::AbstractHeapType::Exn | wast::core::AbstractHeapType::NoExn,
                ..
            }),
            WasmValue::ExnRef(None),
        ) => true,
        (
            Some(wast::core::HeapType::Abstract {
                ty:
                    wast::core::AbstractHeapType::Any
                    | wast::core::AbstractHeapType::Eq
                    | wast::core::AbstractHeapType::Struct
                    | wast::core::AbstractHeapType::Array
                    | wast::core::AbstractHeapType::I31
                    | wast::core::AbstractHeapType::None,
                ..
            }),
            WasmValue::ExternRef(None) | WasmValue::I31Ref(None),
        ) => true,
        (Some(wast::core::HeapType::Concrete(_)), WasmValue::FuncRef(None)) => true,
        _ => false,
    }
}

fn nan_f32(pattern: &NanPattern<wast::token::F32>, bits: u32) -> bool {
    match pattern {
        NanPattern::CanonicalNan => bits & 0x7fff_ffff == 0x7fc0_0000,
        NanPattern::ArithmeticNan => bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x0040_0000 != 0,
        NanPattern::Value(value) => value.bits == bits,
    }
}

fn nan_f64(pattern: &NanPattern<wast::token::F64>, bits: u64) -> bool {
    match pattern {
        NanPattern::CanonicalNan => bits & 0x7fff_ffff_ffff_ffff == 0x7ff8_0000_0000_0000,
        NanPattern::ArithmeticNan => {
            bits & 0x7ff0_0000_0000_0000 == 0x7ff0_0000_0000_0000
                && bits & 0x0008_0000_0000_0000 != 0
        }
        NanPattern::Value(value) => value.bits == bits,
    }
}

fn import_modules(bytes: &[u8]) -> Vec<String> {
    let mut modules = Vec::new();
    for payload in Parser::new(0).parse_all(bytes).flatten() {
        if let Payload::ImportSection(reader) = payload {
            modules.extend(
                reader
                    .into_imports()
                    .flatten()
                    .map(|import| import.module.to_string()),
            );
        }
    }
    modules
}

fn trap_matches(got: &str, expected: &str) -> bool {
    got.contains(expected) || expected.contains(got)
}

fn unimplemented(line: usize, kind: &str, reason: &str) -> DirectiveResult {
    DirectiveResult {
        line,
        kind: kind.into(),
        passed: false,
        expected: kind.into(),
        got: format!("unimplemented: {reason}"),
    }
}

fn fail(line: usize, kind: String, expected: &str, got: &str) -> DirectiveResult {
    DirectiveResult {
        line,
        kind,
        passed: false,
        expected: expected.into(),
        got: got.into(),
    }
}
