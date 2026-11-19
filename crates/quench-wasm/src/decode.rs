//! The one validation of a Wasm binary: wasmparser's `Validator`, plus the two rules it does not
//! state (custom-descriptor subtyping and branch-hint targets), with spec-suite per-path features.

use std::ops::Range;
use std::path::Path;

use wasmparser::{
    BinaryReader, FunctionBody, KnownCustom, Operator, Parser, Payload, TypeRef, Validator,
    WasmFeatures,
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

/// Decode `bytes` then validate with `features`.
pub fn inspect_binary_with(bytes: &[u8], features: WasmFeatures) -> ModuleStatus {
    let mut parser = Parser::new(0);
    parser.set_features(features);
    for payload in parser.parse_all(bytes) {
        if let Err(error) = payload {
            return ModuleStatus::ParseError(error.to_string());
        }
    }
    match validate_with(bytes, features) {
        Ok(_) => ModuleStatus::Valid,
        Err(error) => ModuleStatus::ValidateError(error),
    }
}

/// The single validation: wasmparser's `Validator`, then the rules it leaves out.
fn validate_with(bytes: &[u8], features: WasmFeatures) -> Result<(), String> {
    Validator::new_with_features(features)
        .validate_all(bytes)
        .map(drop)
        .map_err(|error| error.to_string())?;
    match extension_error(bytes, features) {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

const DESCRIPTOR_SUBTYPE: &str = "sub type with descriptor does not match super type without descriptor";
const BRANCH_HINT_TARGET: &str = "@metadata.code.branch_hint annotation: invalid target";

/// Two rules wasmparser 0.247 does not check, applied to the same bytes:
/// a `sub` type with a descriptor needs a supertype with one (custom descriptors), and every
/// branch hint names an `if` or `br_if` in a defined function. Both were checked only by the
/// spec runner (quench-wasm-test); they now hold for every module this crate validates.
fn extension_error(bytes: &[u8], features: WasmFeatures) -> Option<String> {
    let mut parser = Parser::new(0);
    parser.set_features(features);
    // Per type: its declared supertype (module index) and whether it has a descriptor.
    let mut types: Vec<(Option<u32>, bool)> = Vec::new();
    let mut imported_functions = 0u32;
    let mut bodies: Vec<Range<usize>> = Vec::new();
    let mut hints: Vec<(u32, u32)> = Vec::new();
    for payload in parser.parse_all(bytes) {
        match payload {
            Ok(Payload::TypeSection(section)) => {
                for rec in section {
                    let Ok(rec) = rec else { return None };
                    for ty in rec.types() {
                        let has_descriptor = ty.composite_type.descriptor_idx.is_some();
                        let supertype = ty.supertype_idx.and_then(|index| index.as_module_index());
                        types.push((supertype, has_descriptor));
                    }
                }
            }
            Ok(Payload::ImportSection(section)) => {
                for group in section {
                    let Ok(group) = group else { return None };
                    for item in group {
                        let Ok((_, import)) = item else { return None };
                        if matches!(import.ty, TypeRef::Func(_) | TypeRef::FuncExact(_)) {
                            imported_functions += 1;
                        }
                    }
                }
            }
            Ok(Payload::CodeSectionEntry(body)) => bodies.push(body.range()),
            Ok(Payload::CustomSection(section)) => {
                if let KnownCustom::BranchHints(functions) = section.as_known() {
                    for function in functions {
                        let Ok(function) = function else { return None };
                        let func = function.func;
                        for hint in function.hints {
                            let Ok(hint) = hint else { return None };
                            hints.push((func, hint.func_offset));
                        }
                    }
                }
            }
            Err(_) => return None,
            Ok(_) => {}
        }
    }

    for (supertype, has_descriptor) in &types {
        if let (Some(parent), true) = (*supertype, *has_descriptor) {
            if types.get(parent as usize).is_some_and(|(_, parent_has)| !parent_has) {
                return Some(DESCRIPTOR_SUBTYPE.to_string());
            }
        }
    }

    for (func, func_offset) in hints {
        let body = func
            .checked_sub(imported_functions)
            .and_then(|defined| bodies.get(defined as usize))
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

