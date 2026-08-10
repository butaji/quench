use hmac::{Hmac, Mac};
use md5::Md5;
use rand::RngCore;
use rquickjs::{function::Func, Context, Runtime};
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};
use sha3::{digest::ExtendableOutput, digest::Update, Shake128, Shake256};
use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;
mod esm;
mod host_context;
macro_rules! bootstrap_parts {
    ($($name:literal),* $(,)?) => {
        &[$(include_str!(concat!("../polyfills/bootstrap-parts/", $name, ".js"))),*]
    };
}
#[rustfmt::skip]
const BOOTSTRAP_PARTS: &[&str] = bootstrap_parts!(
    "timer-validation", "globals", "globals-extra", "globals-tail", "fetch", "promises", "validation", "arraybuffer", "encoding", "encoding-tail", "pool", "copy-head", "copy", "buffer-validation", "views", "allocation",
    "api-head", "api", "api-tail", "api-tail-02", "path", "support", "events-head", "events", "events-readable-tail", "events-writable-tail", "events-duplex-tail", "events-transform-tail", "events-stream-tail", "events-tail", "filesystem-validation", "filesystem-validation-tail",
    "file-descriptors", "filesystem-access-validation", "io", "io-tail", "metadata", "metadata-tail", "filesystem-permissions", "timestamps", "links", "links-tail", "directory-options", "directory", "streams", "stream-classes", "externalizable-strings", "open-validation",
    "write-validation", "truncate-validation", "read-file", "internal-fs-binding", "streams-tail", "writes", "writes-tail", "performance", "formatting-tail", "formatting", "promisify", "errors", "colors", "colors-tail", "format", "crypto-validation",
    "crypto-head", "crypto", "crypto-tail", "random", "crypto-hmac-validation", "core-head", "core", "core-tail", "network-head", "network-socket", "network-socket-tail", "network-blocklist", "network", "network-promises-tail", "network-validation", "filesystem-internals",
    "cluster", "tcp-binding", "context", "compile", "compression-tail", "compression", "compression-tail-02", "dispatch", "zlib", "decoder", "utf8", "codecs", "tls", "tty", "zlib-streams", "iterators",
    "types", "stream-promises", "web-streams", "web-streams-require", "web-streams-blob", "consumers", "punycode", "module", "channel", "domain", "readline-promises", "repl", "constants", "strict", "sys", "trace-events",
    "wasi", "inspector", "args", "text", "callbackify", "abort", "console", "url", "v8", "os", "metrics", "filesystem-constants", "passthrough", "report", "glob", "dns",
    "dgram-head", "dgram", "dgram-tail", "membership", "https", "http2", "reporters", "sqlite", "util", "cluster-runtime", "cluster-api", "worker", "policy", "setup", "defaults", "alias",
    "workers", "cleanup", "process", "child-process", "shared", "surface", "lifecycle", "child-process-events", "child-process-spawn-errors", "child-process-exec-errors", "sync", "exec", "child-process-validation", "constructor", "fork", "child-process-streams",
    "output", "references", "child-process-encoding", "state", "send", "fork-send", "exit", "disposal", "unlink", "flags", "probe", "ppid", "maps", "ref", "target", "listeners",
    "introspection", "vfs-head", "vfs",
);
static MKDTEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = cli_args().into_iter();
    let mode = args.next();
    if mode.as_deref() == Some("--help") || mode.as_deref() == Some("-h") {
        print_help();
        return Ok(());
    }
    if mode.as_deref() == Some("--stage") {
        let stage = args.next().unwrap_or_else(|| "0".into());
        return run_directory(&PathBuf::from(format!("tests/node-compat/stage-{stage}")));
    }
    if mode.as_deref() == Some("--test-dir") {
        let dir = PathBuf::from(
            args.next()
                .unwrap_or_else(|| "tests/node/test/parallel".into()),
        );
        return run_directory(&dir);
    }
    if mode.as_deref() == Some("--reuse-dir") {
        let dir = PathBuf::from(args.next().unwrap_or_else(|| "tests/node-compat".into()));
        return run_directory_reuse(&dir);
    }
    match mode.as_deref() {
        Some("-e") | Some("--eval") => run_source(&args.next().unwrap_or_default()),
        Some(path) => {
            let source = fs::read_to_string(path)?;
            run_source_with_runtime_at_path(
                &source,
                &Runtime::new()?,
                Some(PathBuf::from(path).as_path()),
            )
        }
        None => run_source(""),
    }
}
fn cli_args() -> Vec<String> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    // Fixture flags are Node CLI flags; experimental switches are selected by
    // the JS polyfills and must not be mistaken for the fixture path.
    args.retain(|arg| {
        !arg.starts_with("--")
            || matches!(
                arg.as_str(),
                "--help" | "--stage" | "--test-dir" | "--reuse-dir" | "--eval"
            )
    });
    args
}
fn print_help() {
    println!("quench-node [--stage N|--test-dir DIR|--reuse-dir DIR|-e CODE|SCRIPT]");
    println!("  --reuse-dir reuses one rquickjs runtime with isolated contexts per script");
}
fn run_source(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::new()?;
    run_source_with_runtime(source, &runtime)
}
fn run_source_with_runtime(
    source: &str,
    runtime: &Runtime,
) -> Result<(), Box<dyn std::error::Error>> {
    run_source_with_runtime_at_path(source, runtime, None)
}
fn run_source_with_runtime_at_path(
    source: &str,
    runtime: &Runtime,
    path: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    runtime.set_loader(esm::NodeResolver, esm::NodeLoader);
    let context = Context::full(runtime)?;
    if let Some(path) = path {
        let filename = if path.is_absolute() {
            path.to_path_buf()
        } else {
            env::current_dir()?.join(path)
        };
        let dirname = filename
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        context.with(|ctx| -> rquickjs::Result<()> {
            ctx.globals()
                .set("__filename", filename.to_string_lossy().as_ref())?;
            ctx.globals()
                .set("__dirname", dirname.to_string_lossy().as_ref())?;
            ctx.globals().set(
                "__quench_script_filename",
                filename.to_string_lossy().as_ref(),
            )?;
            let script_name = filename.to_string_lossy().to_string();
            let mut script_args = Vec::new();
            let mut found_script = false;
            for argument in env::args().skip(1) {
                if found_script {
                    if argument == "--" {
                        continue;
                    }
                    script_args.push(argument);
                } else if argument == script_name || argument == path.to_string_lossy() {
                    found_script = true;
                }
            }
            ctx.globals().set("__quench_script_args", script_args)?;
            Ok(())
        })?;
    }
    run_host_context!(context, source)?;
    Ok(())
}
fn run_single_file(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(dir)?;
    match run_source_with_runtime_at_path(&source, &Runtime::new()?, Some(dir)) {
        Ok(()) => {
            println!("ok {}", dir.display());
            Ok(())
        }
        Err(error) => {
            eprintln!("not ok {}: {error:?}", dir.display());
            Err("Node test harness failures".into())
        }
    }
}
fn run_directory(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if dir.is_file() {
        return run_single_file(dir);
    }
    let mut failed = 0;
    let mut total = 0;
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e == "js" || e == "mjs")
        {
            total += 1;
            let source = fs::read_to_string(entry.path())?;
            match run_source_with_runtime_at_path(&source, &Runtime::new()?, Some(entry.path())) {
                Ok(()) => println!("ok {}", entry.path().display()),
                Err(error) => {
                    failed += 1;
                    eprintln!("not ok {}: {error:?}", entry.path().display());
                }
            }
        }
    }
    println!("{total} tests, {} passed, {failed} failed", total - failed);
    if total == 0 {
        Err("Node test harness found no JavaScript tests".into())
    } else if failed == 0 {
        Ok(())
    } else {
        Err("Node test harness failures".into())
    }
}
fn run_directory_reuse(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::new()?;
    let mut failed = 0;
    let mut total = 0;
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e == "js" || e == "mjs")
        {
            total += 1;
            let source = fs::read_to_string(entry.path())?;
            match run_source_with_runtime_at_path(&source, &runtime, Some(entry.path())) {
                Ok(()) => println!("ok {}", entry.path().display()),
                Err(error) => {
                    failed += 1;
                    eprintln!("not ok {}: {error:?}", entry.path().display());
                }
            }
        }
    }
    println!("{total} tests, {} passed, {failed} failed", total - failed);
    if total == 0 {
        Err("Node reusable harness found no JavaScript tests".into())
    } else if failed == 0 {
        Ok(())
    } else {
        Err("Node reusable harness failures".into())
    }
}
#[cfg(test)]
mod tests {
    use super::run_source;
    #[test]
    fn evaluates_javascript_source() {
        run_source("if (1 + 1 !== 2) throw new Error('bad arithmetic');").unwrap();
    }
    #[test]
    fn loads_node_compatibility_globals() {
        run_source("if (typeof Buffer !== 'function') throw new Error('Buffer missing');").unwrap();
    }
}
