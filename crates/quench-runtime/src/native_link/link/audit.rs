//! The pre-publication link audit: what an execution-only image must not name, and that its
//! semantic exports are exactly its object's imports.

use super::*;
use std::collections::BTreeMap;

const BACKEND_COMPILER_SYMBOLS: &[&str] = &[
    "cranelift_native", "cranelift_object", "cranelift_module", "target_isa_for",
];
const JS_SOURCE_COMPILER_SYMBOLS: &[&str] = &[
    "FunctionCompiler", "specialize_host_script", "specialize_with_mode",
    "specialize_module_with_atom_prefix", "specialize_eval_with_context",
    "specialize_function_constructor",
];
const SYNTAX_FRONTEND_SYMBOLS: &[&str] = &["oxc_parser::", "oxc_semantic::"];
const INTERPRETER_DISPATCH_SYMBOLS: &[&str] = &[
    "::run_fast_lane", "::derive_lane_view", "::js_step_callout",
    "::step_wasm", "::js_tier_frame_entry",
];

#[cfg(test)]
pub(super) fn is_interpreter_symbol(symbol: &str) -> bool {
    INTERPRETER_DISPATCH_SYMBOLS.iter().any(|name| symbol.contains(name))
}

#[cfg(test)]
pub(super) fn is_compiler_symbol(symbol: &str) -> bool {
    compiler_symbol_patterns(symbol).next().is_some()
}

fn backend_compiler_patterns(symbol: &str) -> impl Iterator<Item = &'static str> + '_ {
    BACKEND_COMPILER_SYMBOLS.iter().copied().filter(|name| symbol.contains(name))
}

fn compiler_symbol_patterns(symbol: &str) -> impl Iterator<Item = &'static str> + '_ {
    backend_compiler_patterns(symbol)
        .chain(SYNTAX_FRONTEND_SYMBOLS.iter().copied())
        .chain(JS_SOURCE_COMPILER_SYMBOLS.iter().copied().filter(|_| symbol.contains("quench_runtime")))
        .filter(|name| symbol.contains(name))
}

/// One representative per forbidden mechanism exposes independent retention
/// roots in one link, without repeating every monomorph of the same mechanism.
pub(super) fn retained_execution_support(symbols: &str) -> BTreeMap<&'static str, &str> {
    retained(symbols, false)
}

/// A pure-AOT source image interprets code it compiles at run time: its front end and
/// interpreter are expected, and only the native compiler's symbols are refused.
pub(super) fn retained_backend_compiler(symbols: &str) -> BTreeMap<&'static str, &str> {
    retained(symbols, true)
}

fn retained(symbols: &str, source_frontend: bool) -> BTreeMap<&'static str, &str> {
    let mut retained = BTreeMap::new();
    for line in symbols.lines() {
        let patterns: Vec<&'static str> = if source_frontend {
            backend_compiler_patterns(line).collect()
        } else {
            compiler_symbol_patterns(line).chain(
                INTERPRETER_DISPATCH_SYMBOLS.iter().copied().filter(|name| line.contains(name)),
            ).collect()
        };
        for pattern in patterns {
            retained.entry(pattern).or_insert(line);
        }
    }
    retained
}

pub(super) fn audit(inputs: &NativeImageInputs, executable: &Path, support: NativeImageSupport, source_frontend: bool) -> Result<String, String> {
    let bytes = checked_output(Command::new("nm").arg("-C").arg(executable), "native symbol audit")?;
    let symbols = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    audit_retained_execution(&symbols, support, source_frontend)?;
    audit_exports(inputs, &symbols)?;
    Ok(symbols)
}

/// Refuse an execution-only image that still names the compiler or the interpreter. The
/// mechanism's first line names its retention root; `QUENCH_AUDIT_WARN` publishes the image
/// anyway, so its footprint can be measured while the roots are being cut.
fn audit_retained_execution(symbols: &str, support: NativeImageSupport, source_frontend: bool) -> Result<(), String> {
    // Full support runs its own compiler and interpreter; only an execution-only image is audited.
    if support.allows_dynamic_source() {
        return Ok(());
    }
    // Inspect whole demangled names: Rust types may contain spaces, and a required core generic
    // emitted by OXC does not itself retain an OXC operation.
    let retained = if source_frontend { retained_backend_compiler(symbols) } else { retained_execution_support(symbols) };
    if retained.is_empty() {
        return Ok(());
    }
    let examples = retained.into_iter().map(|(mechanism, line)| format!("{mechanism}: {line}"))
        .collect::<Vec<_>>().join("\n");
    if std::env::var_os("QUENCH_AUDIT_WARN").is_none() {
        return Err(format!("native image retained compilation or interpretation support:\n{examples}"));
    }
    eprintln!("warning: native image retained compilation or interpretation support:\n{examples}");
    Ok(())
}

/// The exported semantic operations must be exactly the object's imports.
fn audit_exports(inputs: &NativeImageInputs, symbols: &str) -> Result<(), String> {
    let mut exports = BTreeSet::new();
    for line in symbols.lines() {
        let mut fields = line.split_whitespace().rev();
        let Some(symbol) = fields.next() else { continue; };
        let symbol = symbol.trim_start_matches('_');
        if fields.next() == Some("T") && symbol.starts_with(crate::NATIVE_JS_OPERATION_PREFIX) {
            exports.insert(symbol.to_owned());
        }
    }
    let expected: BTreeSet<_> = fs::read_to_string(inputs.required_exports())
        .map_err(|error| error.to_string())?
        .lines()
        .filter(|name| name.starts_with(crate::NATIVE_JS_OPERATION_PREFIX))
        .map(str::to_owned)
        .collect();
    if exports != expected {
        return Err("native semantic helper exports do not match object imports".into());
    }
    Ok(())
}
