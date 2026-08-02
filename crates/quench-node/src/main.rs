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

const BOOTSTRAP: &str = include_str!("../polyfills/bootstrap.js");
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
            "__quench_cwd_get",
            Func::from(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).to_string_lossy().into_owned()),
        )?;
        ctx.globals().set(
            "__quench_chdir",
            Func::from(|path: String| -> rquickjs::Result<()> {
                std::env::set_current_dir(path).map_err(|_| rquickjs::Error::new_from_js("process", "chdir failed"))
            }),
        )?;
        ctx.globals().set(
            "__quench_umask",
            Func::from(|mask: Option<u32>| -> u32 {
                #[cfg(unix)]
                unsafe {
                    let current = libc::umask(mask.unwrap_or(0o022) as libc::mode_t);
                    if mask.is_none() { libc::umask(current); }
                    current as u32
                }
                #[cfg(not(unix))]
                { mask.unwrap_or(0o022) }
            }),
        )?;
        ctx.globals().set(
            "__quench_env_get",
            Func::from(|key: String| std::env::var(key).ok()),
        )?;
        ctx.globals().set("__quench_env_set", Func::from(|key: String, value: String| { std::env::set_var(key, value); }))?;
        ctx.globals().set("__quench_env_delete", Func::from(|key: String| { std::env::remove_var(key); }))?;
        ctx.globals().set(
            "__quench_console_write",
            Func::from(|line: String| {
                println!("{line}");
            }),
        )?;
        ctx.globals().set(
            "__quench_now_ns",
            Func::from(|| {
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos().to_string()
            }),
        )?;
        ctx.globals().set(
            "__quench_sleep_ms",
            Func::from(|milliseconds: u64| {
                std::thread::sleep(std::time::Duration::from_millis(milliseconds.min(60_000)));
            }),
        )?;
        ctx.globals().set("__quench_pid", std::process::id())?;
        ctx.globals().set("__quench_exec_path", std::env::current_exe().unwrap_or_else(|_| PathBuf::from("quench-node")).to_string_lossy().into_owned())?;
        ctx.globals().set("__filename", std::env::current_exe().unwrap_or_else(|_| PathBuf::from("quench-node")).to_string_lossy().into_owned())?;
        ctx.globals().set("__quench_argv", env::args().collect::<Vec<String>>())?;
        ctx.globals().set("__quench_env_keys", std::env::vars().map(|(key, _)| key).collect::<Vec<String>>())?;
        ctx.globals().set("__quench_platform", std::env::consts::OS)?;
        ctx.globals().set("__quench_arch", std::env::consts::ARCH)?;
        ctx.globals().set("__quench_tmpdir", std::env::temp_dir().to_string_lossy().into_owned())?;
        ctx.globals().set("__quench_homedir", std::env::var("HOME").unwrap_or_else(|_| "/".into()))?;
        ctx.globals().set("__quench_hostname", hostname::get().map(|v| v.to_string_lossy().into_owned()).unwrap_or_else(|_| "quench-node".into()))?;
        ctx.globals().set("__quench_cpu_count", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1))?;
        ctx.globals().set(
            "__quench_ppid",
            {
                #[cfg(unix)]
                { unsafe { libc::getppid() as u32 } }
                #[cfg(not(unix))]
                { 0u32 }
            },
        )?;
        ctx.globals().set("__quench_getuid", {
            #[cfg(unix)] { Some(unsafe { libc::getuid() as u32 }) }
            #[cfg(not(unix))] { None::<u32> }
        })?;
        ctx.globals().set("__quench_geteuid", {
            #[cfg(unix)] { Some(unsafe { libc::geteuid() as u32 }) }
            #[cfg(not(unix))] { None::<u32> }
        })?;
        ctx.globals().set("__quench_getgid", {
            #[cfg(unix)] { Some(unsafe { libc::getgid() as u32 }) }
            #[cfg(not(unix))] { None::<u32> }
        })?;
        ctx.globals().set("__quench_getegid", {
            #[cfg(unix)] { Some(unsafe { libc::getegid() as u32 }) }
            #[cfg(not(unix))] { None::<u32> }
        })?;
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
            "__quench_sha256_bytes",
            Func::from(|value: Vec<u8>| -> Vec<u8> {
                Sha256::digest(value).to_vec()
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
            "__quench_random_bytes",
            Func::from(|size: u64| -> Vec<u8> {
                let mut bytes = vec![0u8; size.min(16 * 1024 * 1024) as usize];
                rand::thread_rng().fill_bytes(&mut bytes);
                bytes
            }),
        )?;
        ctx.globals().set(
            "__quench_fs_mkdtemp",
            Func::from(|prefix: String| -> rquickjs::Result<String> {
                let root = std::env::temp_dir();
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos() as usize;
                let sequence = MKDTEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                for attempt in 0..100 {
                    let suffix = (stamp.wrapping_add(sequence).wrapping_add(attempt)) % 1_000_000;
                    let path = root.join(format!("{prefix}{suffix:06}"));
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
        ctx.globals().set("__quench_fs_truncate", Func::from(|path: String, length: u64| -> rquickjs::Result<()> {
            fs::OpenOptions::new().write(true).open(path).and_then(|file| file.set_len(length)).map_err(|_| rquickjs::Error::new_from_js("fs", "truncate failed"))
        }))?;
        ctx.globals().set("__quench_fs_read_hex", Func::from(|path: String| -> rquickjs::Result<String> {
            fs::read(path).map(|bytes| bytes.iter().map(|byte| format!("{byte:02x}")).collect()).map_err(|_| rquickjs::Error::new_from_js("fs", "readFileSync failed"))
        }))?;
        ctx.globals().set("__quench_fs_read_bytes", Func::from(|path: String| -> rquickjs::Result<Vec<u8>> {
            fs::read(path).map_err(|_| rquickjs::Error::new_from_js("fs", "readFileSync failed"))
        }))?;
        ctx.globals().set("__quench_fs_read_range_hex", Func::from(|path: String, position: u64, length: u64| -> rquickjs::Result<String> {
            let mut file = fs::File::open(path).map_err(|_| rquickjs::Error::new_from_js("fs", "read failed"))?;
            file.seek(SeekFrom::Start(position)).map_err(|_| rquickjs::Error::new_from_js("fs", "read failed"))?;
            let mut bytes = vec![0; length.min(1024 * 1024) as usize];
            let count = file.read(&mut bytes).map_err(|_| rquickjs::Error::new_from_js("fs", "read failed"))?;
            bytes.truncate(count);
            Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
        }))?;
        ctx.globals().set("__quench_fs_read_range_bytes", Func::from(|path: String, position: u64, length: u64| -> rquickjs::Result<Vec<u8>> {
            let mut file = fs::File::open(path).map_err(|_| rquickjs::Error::new_from_js("fs", "read failed"))?;
            file.seek(SeekFrom::Start(position)).map_err(|_| rquickjs::Error::new_from_js("fs", "read failed"))?;
            let mut bytes = vec![0; length.min(1024 * 1024) as usize];
            let count = file.read(&mut bytes).map_err(|_| rquickjs::Error::new_from_js("fs", "read failed"))?;
            bytes.truncate(count);
            Ok(bytes)
        }))?;
        ctx.globals().set("__quench_fs_write_hex", Func::from(|path: String, hex: String| -> rquickjs::Result<()> {
            let bytes: Result<Vec<u8>, _> = hex.as_bytes().chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16)).collect();
            fs::write(path, bytes.map_err(|_| rquickjs::Error::new_from_js("fs", "writeFileSync failed"))?).map_err(|_| rquickjs::Error::new_from_js("fs", "writeFileSync failed"))
        }))?;
        ctx.globals().set("__quench_fs_write_bytes", Func::from(|path: String, bytes: Vec<u8>| -> rquickjs::Result<()> {
            fs::write(path, bytes).map_err(|_| rquickjs::Error::new_from_js("fs", "writeFileSync failed"))
        }))?;
        ctx.globals().set(
            "__quench_fs_open",
            Func::from(|path: String, flags: String| -> rquickjs::Result<u32> {
                use std::fs::OpenOptions;
                let mut options = OpenOptions::new();
                if flags.starts_with('r') { options.read(true); } else { options.create(true).write(true); if flags.starts_with('w') { options.truncate(true); } }
                options.open(path)
                    .map(|_| 1)
                    .map_err(|_| rquickjs::Error::new_from_js("fs", "openSync failed"))
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
        ctx.globals().set("__quench_fs_link_kind", Func::from(|path: String| -> rquickjs::Result<String> {
            let metadata = fs::symlink_metadata(path).map_err(|_| rquickjs::Error::new_from_js("fs", "lstatSync failed"))?;
            Ok(if metadata.file_type().is_symlink() { "symlink".into() } else if metadata.is_file() { "file".into() } else if metadata.is_dir() { "directory".into() } else { "other".into() })
        }))?;
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
            "__quench_fs_append_bytes",
            Func::from(|path: String, data: Vec<u8>| -> rquickjs::Result<()> {
                use std::io::Write;
                let mut file = fs::OpenOptions::new().create(true).append(true).open(path)
                    .map_err(|_| rquickjs::Error::new_from_js("fs", "appendFileSync failed"))?;
                file.write_all(&data).map_err(|_| rquickjs::Error::new_from_js("fs", "appendFileSync failed"))
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
        ctx.globals().set(
            "__quench_fs_symlink",
            Func::from(|target: String, link: String| -> rquickjs::Result<()> {
                std::os::unix::fs::symlink(target, link).map_err(|_| rquickjs::Error::new_from_js("fs", "symlinkSync failed"))
            }),
        )?;
        ctx.globals().set("__quench_fs_link", Func::from(|source: String, destination: String| -> rquickjs::Result<()> {
            fs::hard_link(source, destination).map_err(|_| rquickjs::Error::new_from_js("fs", "link failed"))
        }))?;
        ctx.globals().set(
            "__quench_fs_readlink",
            Func::from(|path: String| -> rquickjs::Result<String> {
                std::fs::read_link(path).map(|value| value.to_string_lossy().into_owned()).map_err(|_| rquickjs::Error::new_from_js("fs", "readlinkSync failed"))
            }),
        )?;
        ctx.eval::<(), _>(BOOTSTRAP.as_bytes())?;
        let wrapped = format!("try {{\n{source}\n}} catch (error) {{ globalThis.__quench_last_error = error && error.stack ? `${{error.name}}: ${{error.message}}\\n${{error.stack}}` : String(error); throw error; }}");
        ctx.eval::<(), _>(wrapped.as_bytes()).map_err(|error| {
            let detail = ctx.globals().get::<_, String>("__quench_last_error").unwrap_or_else(|_| format!("{error:?}"));
            eprintln!("JavaScript exception: {detail} ({error:?})");
            error
        })?;
        while ctx.execute_pending_job() {}
        if let Ok(detail) = ctx.globals().get::<_, String>("__quench_async_error") {
            if !detail.is_empty() {
                eprintln!("Asynchronous JavaScript exception: {detail}");
                return Err(rquickjs::Error::new_from_js("async", "exception"));
            }
        }
        ctx.eval::<(), _>(b"try { if (typeof process?.emit === 'function') process.emit('exit', process.exitCode || 0); } catch (error) { globalThis.__quench_exit_error = error && error.stack ? `${error.name}: ${error.message}\\n${error.stack}` : String(error); throw error; }")
            .map_err(|error| {
                let detail = ctx.globals().get::<_, String>("__quench_exit_error").unwrap_or_else(|_| format!("{error:?}"));
                eprintln!("Process exit handler failure: {detail}");
                error
            })?;
        ctx.eval::<(), _>(b"try { globalThis.__quench_verify_calls() } catch (error) { __quench_console_write(String(error)); throw error; }").map_err(|error| {
            eprintln!("Node harness assertion failure: {error:?}");
            error
        })
    })?;
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
            match run_source(&source) {
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
            match run_source_with_runtime(&source, &runtime) {
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
