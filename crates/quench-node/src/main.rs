use std::{
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use rand::RngCore;
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
            "__quench_console_write",
            Func::from(|line: String| {
                println!("{line}");
            }),
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
            "__quench_random_uuid",
            Func::from(|| {
                let mut bytes = [0u8; 16];
                rand::thread_rng().fill_bytes(&mut bytes);
                bytes[6] = (bytes[6] & 0x0f) | 0x40;
                bytes[8] = (bytes[8] & 0x3f) | 0x80;
                format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                    bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15])
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
        ctx.globals().set(
            "__quench_fs_mkdir",
            Func::from(|path: String| -> rquickjs::Result<()> {
                fs::create_dir_all(path).map_err(|_| rquickjs::Error::new_from_js("fs", "mkdirSync failed"))
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_readdir",
            Func::from(|path: String| -> rquickjs::Result<Vec<String>> {
                fs::read_dir(path)
                    .map_err(|_| rquickjs::Error::new_from_js("fs", "readdirSync failed"))?
                    .map(|entry| entry.map(|item| item.file_name().to_string_lossy().into_owned()).map_err(|_| rquickjs::Error::new_from_js("fs", "readdirSync failed")))
                    .collect()
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_remove_dir",
            Func::from(|path: String| -> rquickjs::Result<()> {
                fs::remove_dir_all(path).map_err(|_| rquickjs::Error::new_from_js("fs", "rmdirSync failed"))
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_kind",
            Func::from(|path: String| -> rquickjs::Result<String> {
                let metadata = fs::metadata(path).map_err(|_| rquickjs::Error::new_from_js("fs", "statSync failed"))?;
                Ok(if metadata.is_file() { "file".into() } else if metadata.is_dir() { "directory".into() } else { "other".into() })
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_rename",
            Func::from(|from: String, to: String| -> rquickjs::Result<()> {
                fs::rename(from, to).map_err(|_| rquickjs::Error::new_from_js("fs", "renameSync failed"))
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_unlink",
            Func::from(|path: String| -> rquickjs::Result<()> {
                fs::remove_file(path).map_err(|_| rquickjs::Error::new_from_js("fs", "unlinkSync failed"))
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_copy",
            Func::from(|from: String, to: String| -> rquickjs::Result<()> {
                fs::copy(from, to).map(|_| ()).map_err(|_| rquickjs::Error::new_from_js("fs", "copyFileSync failed"))
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_append",
            Func::from(|path: String, data: String| -> rquickjs::Result<()> {
                use std::io::Write;
                let mut file = fs::OpenOptions::new().create(true).append(true).open(path)
                    .map_err(|_| rquickjs::Error::new_from_js("fs", "appendFileSync failed"))?;
                file.write_all(data.as_bytes()).map_err(|_| rquickjs::Error::new_from_js("fs", "appendFileSync failed"))
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_access",
            Func::from(|path: String| fs::metadata(path).is_ok()),
        )?;
        ctx.globals().set(
            "__quench_fs_realpath",
            Func::from(|path: String| -> rquickjs::Result<String> {
                fs::canonicalize(path)
                    .map(|value| value.to_string_lossy().into_owned())
                    .map_err(|_| rquickjs::Error::new_from_js("fs", "realpathSync failed"))
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_chmod",
            Func::from(|path: String, mode: u32| -> rquickjs::Result<()> {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut permissions = fs::metadata(&path).map_err(|_| rquickjs::Error::new_from_js("fs", "chmodSync failed"))?.permissions();
                    permissions.set_mode(mode);
                    fs::set_permissions(path, permissions).map_err(|_| rquickjs::Error::new_from_js("fs", "chmodSync failed"))?;
                }
                #[cfg(not(unix))]
                let _ = (path, mode);
                Ok(())
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
