//! TypeScript checker facts for the files the host loads. A checker (the `quench-tsrs-worker`
//! sidecar, `tools/quench-tsrs-worker`) answers each function's parameter types, which the
//! compiler joins to the declared parameter facts (`Engine::register_checked_typescript_parameters`).
//!
//! The answers are content-addressed facts files: the first load of a file version starts a
//! detached worker that writes them and compiles without; a later load of the same version
//! installs them before compiling. A file's facts depend on the files it imports too, so a
//! cached answer can be stale; a stale fact costs a fallback at entry, never a wrong result.
//!
//! Opt-in: `QUENCH_TS_CHECKER=<worker binary>`. `QUENCH_TS_FACTS_DIR` sets the cache folder
//! (default `$XDG_CACHE_HOME/quench/ts-facts`, else `~/.cache/quench/ts-facts`), and
//! `QUENCH_TS_CHECKER_WAIT=1` waits for the worker on a cache miss.

mod protocol;

// The worker writes language sets in these bits; they must stay the runtime's `ValueType` bits.
const _: () = {
    use quench_runtime::ValueType as T;
    use protocol::*;
    let pairs = [
        (LANGUAGE_UNDEFINED, T::Undefined), (LANGUAGE_NULL, T::Null), (LANGUAGE_BOOLEAN, T::Boolean),
        (LANGUAGE_NUMBER, T::Number), (LANGUAGE_STRING, T::String), (LANGUAGE_SYMBOL, T::Symbol),
        (LANGUAGE_BIGINT, T::BigInt), (LANGUAGE_OBJECT, T::Object), (LANGUAGE_FUNCTION, T::Function),
    ];
    let mut index = 0;
    while index < pairs.len() {
        assert!(pairs[index].0 == 1 << pairs[index].1 as u16);
        index += 1;
    }
    assert!(LANGUAGE_ANY == u16::MAX);
};

use std::hash::{Hash, Hasher};
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

struct Checker {
    worker: PathBuf,
    facts: PathBuf,
    wait: bool,
}

fn checker() -> Option<&'static Checker> {
    static CHECKER: OnceLock<Option<Checker>> = OnceLock::new();
    CHECKER.get_or_init(|| {
        let worker = PathBuf::from(std::env::var_os("QUENCH_TS_CHECKER").filter(|value| !value.is_empty())?);
        let facts = std::env::var_os("QUENCH_TS_FACTS_DIR").map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(|cache| PathBuf::from(cache).join("quench/ts-facts")))
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/quench/ts-facts")))?;
        let wait = std::env::var_os("QUENCH_TS_CHECKER_WAIT").is_some_and(|value| value == "1");
        Some(Checker { worker, facts, wait })
    }).as_ref()
}

/// Install the checked parameter types of the TypeScript file at `path` for `compiled`, the
/// text the host compiles: the file's text at `body` (after a byte-order mark or `#!` line,
/// `disk_offset_utf16` UTF-16 units into the file), maybe inside a wrapper.
pub(crate) fn prepare(path: &Path, compiled: &str, body: Range<usize>, disk_offset_utf16: u32) {
    let Some(checker) = checker() else { return };
    let Some(text) = compiled.get(body.clone()) else { return };
    let Ok(path) = path.canonicalize() else { return };
    let facts = checker.facts.join(format!("{:016x}.facts", version_key(&path, text)));
    if install(&facts, compiled, body.start as u32) {
        return;
    }
    let Some(sites) = quench_runtime::Engine::typescript_parameter_sites(text) else { return };
    if sites.is_empty() || ask(checker, &path, &facts, text, &sites, disk_offset_utf16).is_err() || !checker.wait {
        return;
    }
    install(&facts, compiled, body.start as u32);
}

fn version_key(path: &Path, text: &str) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    (protocol::PROTOCOL_VERSION, protocol::TYPESCRIPT_VERSION, protocol::TSRS_REVISION, path, text).hash(&mut hasher);
    hasher.finish()
}

/// Register the facts file's answer, its keys moved by `shift` into the compiled text.
fn install(facts: &Path, compiled: &str, shift: u32) -> bool {
    let Ok(text) = std::fs::read_to_string(facts) else { return false };
    let mut lines = text.lines();
    let header = format!("{} {} {}", protocol::FACTS_MAGIC, protocol::PROTOCOL_VERSION, protocol::TSRS_REVISION);
    if lines.next() != Some(header.as_str()) {
        return false;
    }
    let mut checked = quench_runtime::CheckedParameters::default();
    for line in lines {
        let mut numbers = line.split(' ').map(str::parse::<u32>);
        let Some(Ok(key)) = numbers.next() else { return false };
        let Ok(languages) = numbers.map(|number| number.ok().and_then(|n| u16::try_from(n).ok()))
            .collect::<Option<Box<[u16]>>>().ok_or(()) else { return false };
        checked.insert(key + shift, languages);
    }
    quench_runtime::Engine::register_checked_typescript_parameters(compiled, checked);
    true
}

/// Start a worker that writes `facts` for `sites` of `text`, the file's text from
/// `disk_offset_utf16`; waits for it when the checker is set to.
fn ask(
    checker: &Checker, path: &Path, facts: &Path, text: &str, sites: &[quench_runtime::ParameterSite],
    disk_offset_utf16: u32,
) -> std::io::Result<()> {
    let utf16 = utf16_offsets(text, sites);
    let request = serde_json::json!({
        "id": 1,
        "path": path.to_string_lossy(),
        "out": facts.to_string_lossy(),
        "sites": sites.iter().map(|site| serde_json::json!({
            "key": site.formals_start,
            "positions_utf16": site.parameters.iter().map(|&position| disk_offset_utf16 + utf16(position)).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    let mut child = Command::new(&checker.worker)
        .current_dir(path.parent().unwrap_or(Path::new("/")))
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn()?;
    let mut input = child.stdin.take().ok_or(std::io::ErrorKind::BrokenPipe)?;
    writeln!(input, "{request}")?;
    drop(input);
    if checker.wait {
        child.wait()?;
    }
    Ok(())
}

/// UTF-8 offset into `text` -> UTF-16 offset, for the offsets `sites` name.
fn utf16_offsets(text: &str, sites: &[quench_runtime::ParameterSite]) -> impl Fn(u32) -> u32 {
    let mut offsets: Vec<u32> = sites.iter().flat_map(|site| site.parameters.iter().copied()).collect();
    offsets.sort_unstable();
    offsets.dedup();
    let mut converted = Vec::with_capacity(offsets.len());
    let (mut units, mut next) = (0_u32, offsets.iter().peekable());
    for (index, character) in text.char_indices() {
        while next.next_if(|&&offset| offset as usize <= index).is_some() {
            converted.push(units);
        }
        units += character.len_utf16() as u32;
    }
    converted.extend(next.map(|_| units));
    move |offset| offsets.binary_search(&offset).map_or(0, |index| converted[index])
}
