//! Third-party binary parse vs validate, with spec-suite per-path features.

use std::path::Path;

use wasmparser::{Parser, Payload, Validator, WasmFeatures};
use wast::core::{FuncKind, Instruction, ModuleField, ModuleKind};
use wast::{QuoteWat, Wat};

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
    let mut validator = Validator::new_with_features(features);
    match validator.validate_all(bytes) {
        Ok(_) => ModuleStatus::Valid,
        Err(error) => ModuleStatus::ValidateError(error.to_string()),
    }
}

/// Decode and validate with Wasm 3.0 core features.
pub fn inspect_binary(bytes: &[u8]) -> ModuleStatus {
    inspect_binary_with(bytes, core_features())
}

/// Encode a wast module then inspect with `features`.
pub fn inspect_quote_with(module: &mut QuoteWat<'_>, features: WasmFeatures) -> ModuleStatus {
    match module.encode() {
        Err(error) => ModuleStatus::ParseError(error.to_string()),
        Ok(bytes) => {
            let status = inspect_binary_with(&bytes, features);
            if matches!(status, ModuleStatus::Valid)
                && features.contains(WasmFeatures::CUSTOM_DESCRIPTORS)
            {
                if let Some(message) = invalid_descriptor_subtype(&bytes) {
                    return ModuleStatus::ValidateError(message);
                }
            }
            status
        }
    }
}

pub(crate) fn invalid_branch_hint_target(module: &QuoteWat<'_>) -> Option<String> {
    let QuoteWat::Wat(Wat::Module(module)) = module else {
        return None;
    };
    let ModuleKind::Text(fields) = &module.kind else {
        return None;
    };
    for field in fields {
        let ModuleField::Func(func) = field else {
            continue;
        };
        let FuncKind::Inline { expression, .. } = &func.kind else {
            continue;
        };
        for hint in expression.branch_hints.iter() {
            let instr = expression.instrs.get(hint.instr_index)?;
            if !matches!(instr, Instruction::If(_) | Instruction::BrIf(_)) {
                return Some("@metadata.code.branch_hint annotation: invalid target".to_string());
            }
        }
    }
    None
}

fn invalid_descriptor_subtype(bytes: &[u8]) -> Option<String> {
    let mut parser = Parser::new(0);
    parser.set_features(WasmFeatures::all());
    let mut types: Vec<(Option<u32>, bool)> = Vec::new();
    for payload in parser.parse_all(bytes) {
        let Ok(Payload::TypeSection(section)) = payload else {
            continue;
        };
        for rec in section {
            let rec = rec.ok()?;
            for ty in rec.types() {
                let has_desc = ty.composite_type.descriptor_idx.is_some();
                let sup = ty.supertype_idx.and_then(|i| i.as_module_index());
                types.push((sup, has_desc));
            }
        }
    }
    for (sup, has_desc) in &types {
        if *has_desc {
            if let Some(idx) = *sup {
                let parent = types.get(idx as usize)?;
                if !parent.1 {
                    return Some(
                        "sub type with descriptor does not match super type without descriptor"
                            .to_string(),
                    );
                }
            }
        }
    }
    None
}
