use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use hmac::{Hmac, Mac};
use md5::Md5;
use rand::RngCore;
use rquickjs::{function::Func, Context, Runtime};
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};
use sha3::{digest::ExtendableOutput, digest::Update, Shake128, Shake256};
use walkdir::WalkDir;

mod esm;
mod host_context;

const BOOTSTRAP_PARTS: &[&str] = &[
    include_str!("../polyfills/bootstrap-parts/timer-validation.js"),
    include_str!("../polyfills/bootstrap-parts/globals.js"),
    include_str!("../polyfills/bootstrap-parts/fetch.js"),
    include_str!("../polyfills/bootstrap-parts/promises.js"),
    include_str!("../polyfills/bootstrap-parts/validation.js"),
    include_str!("../polyfills/bootstrap-parts/arraybuffer.js"),
    include_str!("../polyfills/bootstrap-parts/encoding.js"),
    include_str!("../polyfills/bootstrap-parts/encoding-tail.js"),
    include_str!("../polyfills/bootstrap-parts/pool.js"),
    include_str!("../polyfills/bootstrap-parts/copy-head.js"),
    include_str!("../polyfills/bootstrap-parts/copy.js"),
    include_str!("../polyfills/bootstrap-parts/buffer-validation.js"),
    include_str!("../polyfills/bootstrap-parts/views.js"),
    include_str!("../polyfills/bootstrap-parts/allocation.js"),
    include_str!("../polyfills/bootstrap-parts/api.js"),
    include_str!("../polyfills/bootstrap-parts/path.js"),
    include_str!("../polyfills/bootstrap-parts/support.js"),
    include_str!("../polyfills/bootstrap-parts/events.js"),
    include_str!("../polyfills/bootstrap-parts/filesystem-validation.js"),
    include_str!("../polyfills/bootstrap-parts/filesystem-validation-tail.js"),
    include_str!("../polyfills/bootstrap-parts/file-descriptors.js"),
    include_str!("../polyfills/bootstrap-parts/filesystem-access-validation.js"),
    include_str!("../polyfills/bootstrap-parts/io.js"),
    include_str!("../polyfills/bootstrap-parts/io-tail.js"),
    include_str!("../polyfills/bootstrap-parts/metadata.js"),
    include_str!("../polyfills/bootstrap-parts/metadata-tail.js"),
    include_str!("../polyfills/bootstrap-parts/filesystem-permissions.js"),
    include_str!("../polyfills/bootstrap-parts/timestamps.js"),
    include_str!("../polyfills/bootstrap-parts/links.js"),
    include_str!("../polyfills/bootstrap-parts/links-tail.js"),
    include_str!("../polyfills/bootstrap-parts/directory-options.js"),
    include_str!("../polyfills/bootstrap-parts/directory.js"),
    include_str!("../polyfills/bootstrap-parts/stream-classes.js"),
    include_str!("../polyfills/bootstrap-parts/externalizable-strings.js"),
    include_str!("../polyfills/bootstrap-parts/open-validation.js"),
    include_str!("../polyfills/bootstrap-parts/write-validation.js"),
    include_str!("../polyfills/bootstrap-parts/truncate-validation.js"),
    include_str!("../polyfills/bootstrap-parts/read-file.js"),
    include_str!("../polyfills/bootstrap-parts/internal-fs-binding.js"),
    include_str!("../polyfills/bootstrap-parts/streams.js"),
    include_str!("../polyfills/bootstrap-parts/streams-tail.js"),
    include_str!("../polyfills/bootstrap-parts/writes.js"),
    include_str!("../polyfills/bootstrap-parts/writes-tail.js"),
    include_str!("../polyfills/bootstrap-parts/performance.js"),
    include_str!("../polyfills/bootstrap-parts/formatting-tail.js"),
    include_str!("../polyfills/bootstrap-parts/formatting.js"),
    include_str!("../polyfills/bootstrap-parts/promisify.js"),
    include_str!("../polyfills/bootstrap-parts/errors.js"),
    include_str!("../polyfills/bootstrap-parts/colors.js"),
    include_str!("../polyfills/bootstrap-parts/format.js"),
    include_str!("../polyfills/bootstrap-parts/crypto-validation.js"),
    include_str!("../polyfills/bootstrap-parts/crypto-head.js"),
    include_str!("../polyfills/bootstrap-parts/crypto.js"),
    include_str!("../polyfills/bootstrap-parts/random.js"),
    include_str!("../polyfills/bootstrap-parts/crypto-hmac-validation.js"),
    include_str!("../polyfills/bootstrap-parts/core.js"),
    include_str!("../polyfills/bootstrap-parts/network.js"),
    include_str!("../polyfills/bootstrap-parts/network-validation.js"),
    include_str!("../polyfills/bootstrap-parts/filesystem-internals.js"),
    include_str!("../polyfills/bootstrap-parts/cluster.js"),
    include_str!("../polyfills/bootstrap-parts/tcp-binding.js"),
    include_str!("../polyfills/bootstrap-parts/context.js"),
    include_str!("../polyfills/bootstrap-parts/compile.js"),
    include_str!("../polyfills/bootstrap-parts/compression-tail.js"),
    include_str!("../polyfills/bootstrap-parts/compression.js"),
    include_str!("../polyfills/bootstrap-parts/compression-tail-02.js"),
    include_str!("../polyfills/bootstrap-parts/dispatch.js"),
    include_str!("../polyfills/bootstrap-parts/zlib.js"),
    include_str!("../polyfills/bootstrap-parts/decoder.js"),
    include_str!("../polyfills/bootstrap-parts/utf8.js"),
    include_str!("../polyfills/bootstrap-parts/codecs.js"),
    include_str!("../polyfills/bootstrap-parts/tls.js"),
    include_str!("../polyfills/bootstrap-parts/tty.js"),
    include_str!("../polyfills/bootstrap-parts/zlib-streams.js"),
    include_str!("../polyfills/bootstrap-parts/iterators.js"),
    include_str!("../polyfills/bootstrap-parts/types.js"),
    include_str!("../polyfills/bootstrap-parts/stream-promises.js"),
    include_str!("../polyfills/bootstrap-parts/web-streams.js"),
    include_str!("../polyfills/bootstrap-parts/web-streams-require.js"),
    include_str!("../polyfills/bootstrap-parts/web-streams-blob.js"),
    include_str!("../polyfills/bootstrap-parts/consumers.js"),
    include_str!("../polyfills/bootstrap-parts/punycode.js"),
    include_str!("../polyfills/bootstrap-parts/module.js"),
    include_str!("../polyfills/bootstrap-parts/channel.js"),
    include_str!("../polyfills/bootstrap-parts/domain.js"),
    include_str!("../polyfills/bootstrap-parts/readline-promises.js"),
    include_str!("../polyfills/bootstrap-parts/repl.js"),
    include_str!("../polyfills/bootstrap-parts/constants.js"),
    include_str!("../polyfills/bootstrap-parts/strict.js"),
    include_str!("../polyfills/bootstrap-parts/sys.js"),
    include_str!("../polyfills/bootstrap-parts/trace-events.js"),
    include_str!("../polyfills/bootstrap-parts/wasi.js"),
    include_str!("../polyfills/bootstrap-parts/inspector.js"),
    include_str!("../polyfills/bootstrap-parts/args.js"),
    include_str!("../polyfills/bootstrap-parts/text.js"),
    include_str!("../polyfills/bootstrap-parts/callbackify.js"),
    include_str!("../polyfills/bootstrap-parts/abort.js"),
    include_str!("../polyfills/bootstrap-parts/console.js"),
    include_str!("../polyfills/bootstrap-parts/url.js"),
    include_str!("../polyfills/bootstrap-parts/v8.js"),
    include_str!("../polyfills/bootstrap-parts/os.js"),
    include_str!("../polyfills/bootstrap-parts/metrics.js"),
    include_str!("../polyfills/bootstrap-parts/filesystem-constants.js"),
    include_str!("../polyfills/bootstrap-parts/passthrough.js"),
    include_str!("../polyfills/bootstrap-parts/report.js"),
    include_str!("../polyfills/bootstrap-parts/glob.js"),
    include_str!("../polyfills/bootstrap-parts/dns.js"),
    include_str!("../polyfills/bootstrap-parts/dgram-head.js"),
    include_str!("../polyfills/bootstrap-parts/dgram.js"),
    include_str!("../polyfills/bootstrap-parts/dgram-tail.js"),
    include_str!("../polyfills/bootstrap-parts/membership.js"),
    include_str!("../polyfills/bootstrap-parts/https.js"),
    include_str!("../polyfills/bootstrap-parts/http2.js"),
    include_str!("../polyfills/bootstrap-parts/reporters.js"),
    include_str!("../polyfills/bootstrap-parts/sqlite.js"),
    include_str!("../polyfills/bootstrap-parts/util.js"),
    include_str!("../polyfills/bootstrap-parts/cluster-runtime.js"),
    include_str!("../polyfills/bootstrap-parts/cluster-api.js"),
    include_str!("../polyfills/bootstrap-parts/worker.js"),
    include_str!("../polyfills/bootstrap-parts/policy.js"),
    include_str!("../polyfills/bootstrap-parts/setup.js"),
    include_str!("../polyfills/bootstrap-parts/defaults.js"),
    include_str!("../polyfills/bootstrap-parts/alias.js"),
    include_str!("../polyfills/bootstrap-parts/workers.js"),
    include_str!("../polyfills/bootstrap-parts/cleanup.js"),
    include_str!("../polyfills/bootstrap-parts/process.js"),
    include_str!("../polyfills/bootstrap-parts/child-process.js"),
    include_str!("../polyfills/bootstrap-parts/shared.js"),
    include_str!("../polyfills/bootstrap-parts/surface.js"),
    include_str!("../polyfills/bootstrap-parts/lifecycle.js"),
    include_str!("../polyfills/bootstrap-parts/child-process-events.js"),
    include_str!("../polyfills/bootstrap-parts/child-process-spawn-errors.js"),
    include_str!("../polyfills/bootstrap-parts/child-process-exec-errors.js"),
    include_str!("../polyfills/bootstrap-parts/sync.js"),
    include_str!("../polyfills/bootstrap-parts/exec.js"),
    include_str!("../polyfills/bootstrap-parts/child-process-validation.js"),
    include_str!("../polyfills/bootstrap-parts/constructor.js"),
    include_str!("../polyfills/bootstrap-parts/fork.js"),
    include_str!("../polyfills/bootstrap-parts/child-process-streams.js"),
    include_str!("../polyfills/bootstrap-parts/output.js"),
    include_str!("../polyfills/bootstrap-parts/references.js"),
    include_str!("../polyfills/bootstrap-parts/child-process-encoding.js"),
    include_str!("../polyfills/bootstrap-parts/state.js"),
    include_str!("../polyfills/bootstrap-parts/send.js"),
    include_str!("../polyfills/bootstrap-parts/fork-send.js"),
    include_str!("../polyfills/bootstrap-parts/exit.js"),
    include_str!("../polyfills/bootstrap-parts/disposal.js"),
    include_str!("../polyfills/bootstrap-parts/unlink.js"),
    include_str!("../polyfills/bootstrap-parts/flags.js"),
    include_str!("../polyfills/bootstrap-parts/probe.js"),
    include_str!("../polyfills/bootstrap-parts/ppid.js"),
    include_str!("../polyfills/bootstrap-parts/maps.js"),
    include_str!("../polyfills/bootstrap-parts/ref.js"),
    include_str!("../polyfills/bootstrap-parts/target.js"),
    include_str!("../polyfills/bootstrap-parts/listeners.js"),
    include_str!("../polyfills/bootstrap-parts/introspection.js"),
    include_str!("../polyfills/bootstrap-parts/vfs.js"),
];
static MKDTEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw_args: Vec<String> = env::args().skip(1).collect();
    // Fixture flags are Node CLI flags.  Quench consumes the fixture path
    // itself, so experimental feature switches must not be mistaken for it;
    // their compatibility behavior is selected by the JS polyfills.
    raw_args.retain(|arg| {
        !arg.starts_with("--")
            || matches!(
                arg.as_str(),
                "--help" | "--stage" | "--test-dir" | "--reuse-dir" | "--eval"
            )
    });
    let mut args = raw_args.into_iter();
    let mode = args.next();
    if mode.as_deref() == Some("--help") || mode.as_deref() == Some("-h") {
        println!("quench-node [--stage N|--test-dir DIR|--reuse-dir DIR|-e CODE|SCRIPT]");
        println!("  --reuse-dir reuses one rquickjs runtime with isolated contexts per script");
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
