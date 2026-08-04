use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use rand::RngCore;
use rquickjs::{function::Func, Context, Runtime};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

mod host_context;

const BOOTSTRAP_PARTS: &[&str] = &[
    include_str!("../polyfills/bootstrap-parts/bootstrap-00-globals.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-01-timers-promises.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-02-buffer-input-validation.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-02-buffer-encoding.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-02-buffer-copy.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-02-buffer-validation.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-02-buffer-views.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-02-buffer-allocation.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-03-buffer-api.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-04-path.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-05-events.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-06-filesystem-validation.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-06-filesystem-io.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-07-filesystem-metadata.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-08-filesystem-links.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-09-filesystem-streams.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-10-filesystem-writes.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-11-performance.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-12-util-formatting.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-13-util-colors.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-14-crypto.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-15-require-core.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-15-require-network.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-15-require-cluster.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-15-require-compression.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-15-require-dispatch.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-16-zlib.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-17-string-decoder.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-18-tls.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-19-tty.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-20-zlib-streams.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-21-zlib-iterators.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-22-util-types.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-23-stream-promises.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-24-web-streams.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-25-stream-consumers.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-26-punycode.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-27-module.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-28-diagnostics-channel.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-29-domain.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-30-readline-promises.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-31-repl.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-32-constants.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-33-assert-strict.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-34-sys.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-35-trace-events.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-36-wasi.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-37-inspector.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-38-parse-args.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-39-style-text.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-40-callbackify.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-41-transferable-abort.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-42-console.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-43-url.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-44-v8.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-45-os.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-46-process-metrics.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-47-filesystem-constants.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-48-stream-passthrough.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-49-process-report.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-50-glob.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-51-dns.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-52-dgram.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-53-https.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-54-http2.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-55-test-reporters.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-56-sqlite.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-57-shared-util.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-58-cluster.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-59-cluster-api.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-60-cluster-worker.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-61-cluster-policy.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-62-cluster-setup.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-63-cluster-defaults.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-64-cluster-alias.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-65-cluster-workers.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-66-cluster-cleanup.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-67-cluster-process.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-68-child-process.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-69-child-process-shared.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-70-child-process-surface.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-71-child-process-lifecycle.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-72-child-process-events.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-73-child-process-spawn-errors.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-74-child-process-exec-errors.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-75-child-process-spawn-sync.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-76-child-process-sync-exec.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-77-child-process-validation.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-78-child-process-constructor.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-79-child-process-fork.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-80-child-process-streams.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-81-child-process-output.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-82-child-process-references.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-83-child-process-encoding.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-84-child-process-stream-state.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-85-process-send.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-86-fork-send.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-87-fork-exit.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-88-child-process-disposal.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-89-filesystem-unlink.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-90-stream-flags.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-91-process-probe.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-92-process-ppid.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-95-source-maps.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-96-ref.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-99-event-target.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-100-event-target-listeners.js"),
    include_str!("../polyfills/bootstrap-parts/bootstrap-101-event-emitter-introspection.js"),
];
static MKDTEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw_args: Vec<String> = env::args().skip(1).collect();
    raw_args.retain(|arg| arg != "--experimental-stream-iter");
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
    let source = match mode.as_deref() {
        Some("-e") | Some("--eval") => args.next().unwrap_or_default(),
        Some(path) => fs::read_to_string(path)?,
        None => String::new(),
    };
    run_source(&source)
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

fn run_directory(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
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
