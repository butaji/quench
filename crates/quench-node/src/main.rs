use std::{
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use rquickjs::{function::Func, Context, Runtime};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

const BOOTSTRAP: &str = include_str!("../polyfills/bootstrap.js");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mode = args.next();
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
    let source = match mode.as_deref() {
        Some("-e") | Some("--eval") => args.next().unwrap_or_default(),
        Some(path) => fs::read_to_string(path)?,
        None => String::new(),
    };
    run_source(&source)
}

fn run_source(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::new()?;
    let context = Context::full(&runtime)?;
    context.with(|ctx| -> rquickjs::Result<()> {
        ctx.globals().set(
            "__quench_fs_exists",
            Func::from(|path: String| fs::metadata(path).is_ok()),
        )?;
        ctx.globals().set(
            "__quench_cwd",
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .to_string_lossy()
                .into_owned(),
        )?;
        ctx.globals().set(
            "__quench_env_get",
            Func::from(|key: String| std::env::var(key).ok()),
        )?;
        ctx.globals().set(
            "__quench_sha256",
            Func::from(|value: String| {
                let digest = Sha256::digest(value.as_bytes());
                digest
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_mkdtemp",
            Func::from(|prefix: String| -> rquickjs::Result<String> {
                let root = std::env::temp_dir();
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                for attempt in 0..100 {
                    let path = root.join(format!("{prefix}{stamp}-{attempt}"));
                    if fs::create_dir(&path).is_ok() {
                        return Ok(path.to_string_lossy().into_owned());
                    }
                }
                Err(rquickjs::Error::new_from_js("fs", "mkdtemp failed"))
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_read_file",
            Func::from(|path: String| -> rquickjs::Result<String> {
                fs::read_to_string(path)
                    .map_err(|_| rquickjs::Error::new_from_js("fs", "readFileSync failed"))
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_write_file",
            Func::from(|path: String, data: String| -> rquickjs::Result<()> {
                fs::write(path, data)
                    .map_err(|_| rquickjs::Error::new_from_js("fs", "writeFileSync failed"))
            }),
        )?;
        ctx.eval::<(), _>(BOOTSTRAP.as_bytes())?;
        ctx.eval::<(), _>(source.as_bytes())
    })?;
    Ok(())
}

fn run_directory(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let mut failed = 0;
    let mut total = 0;
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "js") {
            total += 1;
            let source = fs::read_to_string(entry.path())?;
            match run_source(&source) {
                Ok(()) => println!("ok {}", entry.path().display()),
                Err(error) => {
                    failed += 1;
                    eprintln!("not ok {}: {error}", entry.path().display());
                }
            }
        }
    }
    println!("{total} tests, {} passed, {failed} failed", total - failed);
    if failed == 0 {
        Ok(())
    } else {
        Err("Node test harness failures".into())
    }
}
