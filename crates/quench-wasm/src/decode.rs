//! The one validation of a Wasm binary: wasmparser's `Validator`, plus the two rules it does not
//! state (custom-descriptor subtyping and branch-hint targets), with spec-suite per-path features.

use std::ops::Range;
use std::path::Path;

use wasmparser::{
    BinaryReader, FuncValidatorAllocations, FunctionBody, KnownCustom, Operator, Parser, Payload,
    TypeRef, ValidPayload, Validator, WasmFeatures,
};

/// Outcome of inspecting a Wasm binary without executing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleStatus {
    /// The bytes are not a well-formed module (decode/parse failed).
    ParseError(String),
    /// The bytes decoded, then failed type validation.
    ValidateError(String),
    /// Decode and validation both succeeded.
    Valid,
}

/// Wasm 3.0 core minus extra proposals the amalgamated suite tests *without*.
///
/// `WasmFeatures::all()` / `WASM3` with threads+stack-switching+descriptors
/// enabled makes modules the suite marks invalid validate successfully.
pub fn core_features() -> WasmFeatures {
    WasmFeatures::WASM2
        | WasmFeatures::GC
        | WasmFeatures::TAIL_CALL
        | WasmFeatures::EXTENDED_CONST
        | WasmFeatures::FUNCTION_REFERENCES
        | WasmFeatures::MULTI_MEMORY
        | WasmFeatures::RELAXED_SIMD
        | WasmFeatures::EXCEPTIONS
        | WasmFeatures::MEMORY64
}

/// What the JavaScript API (`WebAssembly.validate`, `compile`, `Module`) accepts: the core set
/// plus threads and the legacy exception-handling proposal (`try`, `catch`, `catch_all`, `delegate`,
/// `rethrow`), which V8 and JavaScriptCore still ship and toolchains such as .NET's still
/// emit (Node v26.10.0 validates such a module). The official suite tests core without it.
pub fn js_api_features() -> WasmFeatures {
    core_features() | WasmFeatures::LEGACY_EXCEPTIONS | WasmFeatures::THREADS
}

/// Features the official suite uses for a `.wast` path.
pub fn features_for_path(path: impl AsRef<Path>) -> WasmFeatures {
    let path = path.as_ref();
    let text = path.to_string_lossy().replace('\\', "/");
    match proposal_from_path(&text) {
        Some("threads") => (WasmFeatures::WASM2 | WasmFeatures::THREADS)
            .difference(WasmFeatures::REFERENCE_TYPES)
            .difference(WasmFeatures::MULTI_MEMORY),
        Some("custom-page-sizes") => {
            core_features() | WasmFeatures::CUSTOM_PAGE_SIZES | WasmFeatures::MEMORY64
        }
        Some("custom-descriptors") => core_features() | WasmFeatures::CUSTOM_DESCRIPTORS,
        Some("wide-arithmetic") => core_features() | WasmFeatures::WIDE_ARITHMETIC,
        Some(_) => core_features(),
        None if text.contains("/legacy/") => core_features() | WasmFeatures::LEGACY_EXCEPTIONS,
        None => core_features(),
    }
}

fn proposal_from_path(path: &str) -> Option<&str> {
    let mut parts = path.split('/');
    while let Some(part) = parts.next() {
        if part == "proposals" {
            return parts.next();
        }
    }
    None
}

/// Decode `bytes` then validate with `features`. A module that does not parse is a `ParseError`;
/// one that parses but fails validation is a `ValidateError`. The parse-only pass runs only where
/// validation failed, to tell the two apart.
pub fn inspect_binary_with(bytes: &[u8], features: WasmFeatures) -> ModuleStatus {
    match validate_with(bytes, features) {
        Ok(_) => ModuleStatus::Valid,
        Err(error) => match first_parse_error(bytes, features) {
            Some(parse_error) => ModuleStatus::ParseError(parse_error),
            None => ModuleStatus::ValidateError(error),
        },
    }
}

/// The first error of parsing `bytes` alone, if any.
fn first_parse_error(bytes: &[u8], features: WasmFeatures) -> Option<String> {
    let mut parser = Parser::new(0);
    parser.set_features(features);
    parser.parse_all(bytes).find_map(|payload| payload.err().map(|error| error.to_string()))
}

/// The single validation: one parse that feeds wasmparser's `Validator` and gathers the facts
/// the rules it leaves out need, then those rules. Function bodies validate after the parse, as
/// `Validator::validate_all` orders them, so a parse error wins over a body's error.
fn validate_with(bytes: &[u8], features: WasmFeatures) -> Result<(), String> {
    let mut parser = Parser::new(0);
    parser.set_features(features);
    let mut validator = Validator::new_with_features(features);
    let mut facts = ExtensionFacts::default();
    let mut functions = Vec::new();
    for payload in parser.parse_all(bytes) {
        let payload = payload.map_err(|error| error.to_string())?;
        if let ValidPayload::Func(function, body) = validator.payload(&payload).map_err(|error| error.to_string())? {
            functions.push((function, body));
        }
        facts.record(&payload);
    }
    let mut allocations = FuncValidatorAllocations::default();
    for (function, body) in functions {
        let mut body_validator = function.into_validator(allocations);
        body_validator.validate(&body).map_err(|error| error.to_string())?;
        allocations = body_validator.into_allocations();
    }
    match facts.error(bytes) {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

const DESCRIPTOR_SUBTYPE: &str = "sub type with descriptor does not match super type without descriptor";
const BRANCH_HINT_TARGET: &str = "@metadata.code.branch_hint annotation: invalid target";

/// What the rules wasmparser 0.247 does not check need from a module, gathered as its payloads
/// are validated.
#[derive(Default)]
struct ExtensionFacts {
    /// Per type: its declared supertype (module index) and whether it has a descriptor.
    types: Vec<(Option<u32>, bool)>,
    imported_functions: u32,
    bodies: Vec<Range<usize>>,
    hints: Vec<(u32, u32)>,
    /// Set when a payload's contents do not read back: the rules then state no error.
    unreadable: bool,
}

impl ExtensionFacts {
    /// Records the facts of `payload`, which has validated.
    fn record(&mut self, payload: &Payload<'_>) {
        match payload {
            Payload::TypeSection(section) => {
                for rec in section.clone() {
                    let Ok(rec) = rec else { self.unreadable = true; return };
                    for ty in rec.types() {
                        let has_descriptor = ty.composite_type.descriptor_idx.is_some();
                        let supertype = ty.supertype_idx.and_then(|index| index.as_module_index());
                        self.types.push((supertype, has_descriptor));
                    }
                }
            }
            Payload::ImportSection(section) => {
                for group in section.clone() {
                    let Ok(group) = group else { self.unreadable = true; return };
                    for item in group {
                        let Ok((_, import)) = item else { self.unreadable = true; return };
                        if matches!(import.ty, TypeRef::Func(_) | TypeRef::FuncExact(_)) {
                            self.imported_functions += 1;
                        }
                    }
                }
            }
            Payload::CodeSectionEntry(body) => self.bodies.push(body.range()),
            Payload::CustomSection(section) => {
                if let KnownCustom::BranchHints(functions) = section.as_known() {
                    for function in functions {
                        let Ok(function) = function else { self.unreadable = true; return };
                        let func = function.func;
                        for hint in function.hints {
                            let Ok(hint) = hint else { self.unreadable = true; return };
                            self.hints.push((func, hint.func_offset));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// The first rule the module breaks, if any.
    fn error(self, bytes: &[u8]) -> Option<String> {
        if self.unreadable {
            return None;
        }
        let types = &self.types;
        for (supertype, has_descriptor) in types {
            if let (Some(parent), true) = (*supertype, *has_descriptor) {
                if types.get(parent as usize).is_some_and(|(_, parent_has)| !parent_has) {
                    return Some(DESCRIPTOR_SUBTYPE.to_string());
                }
            }
        }

        for (func, func_offset) in self.hints {
            let body = func
                .checked_sub(self.imported_functions)
                .and_then(|defined| self.bodies.get(defined as usize))
                .cloned();
            let target = body
                .as_ref()
                .and_then(|body| body.start.checked_add(func_offset as usize));
            let conditional = match (body, target) {
                (Some(body), Some(target)) => conditional_at(bytes, body, target),
                _ => false,
            };
            if !conditional {
                return Some(BRANCH_HINT_TARGET.to_string());
            }
        }
        None
    }
}

/// Whether the operator at absolute offset `target` in the body at `range` is `if` or `br_if`.
fn conditional_at(bytes: &[u8], range: Range<usize>, target: usize) -> bool {
    let Some(data) = bytes.get(range.clone()) else { return false };
    let body = FunctionBody::new(BinaryReader::new(data, range.start));
    let Ok(reader) = body.get_operators_reader() else { return false };
    for item in reader.into_iter_with_offsets() {
        let Ok((operator, offset)) = item else { return false };
        if offset == target {
            return matches!(operator, Operator::If { .. } | Operator::BrIf { .. });
        }
        if offset > target {
            return false;
        }
    }
    false
}

/// The JavaScript API's validation result, without lowering or instantiation.
pub fn validate_binary(bytes: &[u8]) -> bool {
    validate_with(bytes, js_api_features()).is_ok()
}

/// Decode and validate with Wasm 3.0 core features.
pub fn inspect_binary(bytes: &[u8]) -> ModuleStatus {
    inspect_binary_with(bytes, js_api_features())
}

