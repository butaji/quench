#[macro_export]
macro_rules! run_host_context {
    ($context:expr, $source:expr) => {
    $context.with(|ctx| -> rquickjs::Result<()> {
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
            "__quench_zlib_deflate",
            Func::from(|value: Vec<u8>| -> rquickjs::Result<Vec<u8>> {
                use flate2::write::DeflateEncoder;
                use flate2::Compression;
                use std::io::Write;
                let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
                let write_result = encoder.write_all(&value);
                if let Err(e) = write_result {
                    return Err(rquickjs::Error::new_from_js_message("zlib", "deflate failed", e.to_string()));
                }
                match encoder.finish() {
                    Ok(v) => Ok(v),
                    Err(e) => Err(rquickjs::Error::new_from_js_message("zlib", "deflate finish failed", e.to_string())),
                }
            }),
        )?;
        ctx.globals().set(
            "__quench_zlib_inflate",
            Func::from(|value: Vec<u8>| -> rquickjs::Result<Vec<u8>> {
                use flate2::read::DeflateDecoder;
                use std::io::Read;
                let mut decoder = DeflateDecoder::new(&value[..]);
                let mut out = Vec::new();
                match decoder.read_to_end(&mut out) {
                    Ok(_) => Ok(out),
                    Err(e) => Err(rquickjs::Error::new_from_js_message("zlib", "inflate failed", e.to_string())),
                }
            }),
        )?;
        ctx.globals().set(
            "__quench_zlib_gzip",
            Func::from(|value: Vec<u8>| -> rquickjs::Result<Vec<u8>> {
                use flate2::write::GzEncoder;
                use flate2::Compression;
                use std::io::Write;
                let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                let write_result = encoder.write_all(&value);
                if let Err(e) = write_result {
                    return Err(rquickjs::Error::new_from_js_message("zlib", "gzip failed", e.to_string()));
                }
                match encoder.finish() {
                    Ok(v) => Ok(v),
                    Err(e) => Err(rquickjs::Error::new_from_js_message("zlib", "gzip finish failed", e.to_string())),
                }
            }),
        )?;
        ctx.globals().set(
            "__quench_zlib_gunzip",
            Func::from(|value: Vec<u8>| -> rquickjs::Result<Vec<u8>> {
                use flate2::read::GzDecoder;
                use std::io::Read;
                let mut decoder = GzDecoder::new(&value[..]);
                let mut out = Vec::new();
                match decoder.read_to_end(&mut out) {
                    Ok(_) => Ok(out),
                    Err(e) => Err(rquickjs::Error::new_from_js_message("zlib", "gunzip failed", e.to_string())),
                }
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
        let mut fragment_loader = String::new();
        for source_line in BOOTSTRAP_PARTS.iter().flat_map(|part| part.lines()) {
            fragment_loader.push_str(source_line);
            fragment_loader.push('\n');
        }
        let loader_literal = fragment_loader
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        let bootstrap = format!(
            "try {{ eval(\"{loader_literal}\"); }} catch (error) {{ __quench_console_write(`bootstrap: ${{error?.name}}: ${{error?.message}}\\n${{error?.stack || error}}`); throw error; }}"
        );
        ctx.eval::<(), _>(bootstrap.as_bytes()).map_err(|error| {
            eprintln!("Bootstrap JavaScript exception: {error:?}");
            error
        })?;
        let bootstrap_source = ctx
            .eval::<String, _>(b"globalThis.__quench_bootstrap_fragments.join('\\n')")
            .map_err(|error| {
                eprintln!("Bootstrap fragment assembly failed: {error:?}");
                error
            })?;
        let bootstrap_literal = bootstrap_source
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        let bootstrap = format!(
            "try {{ eval(\"{bootstrap_literal}\"); }} catch (error) {{ __quench_console_write(`bootstrap: ${{error?.name}}: ${{error?.message}}\\n${{error?.stack || error}}`); throw error; }}"
        );
        ctx.eval::<(), _>(bootstrap.as_bytes()).map_err(|error| {
            eprintln!("Bootstrap JavaScript exception: {error:?}");
            error
        })?;
        ctx.eval::<(), _>(b"if (globalThis.process) { globalThis.process.getActiveResourcesInfo = () => []; globalThis.process.availableMemory = () => Number.MAX_SAFE_INTEGER; globalThis.process.setSourceMapsEnabled = () => undefined; globalThis.process.sourceMapsEnabled = false; globalThis.process.debugPort = 9229; globalThis.process.release = { name: 'node', sourceUrl: '', headersUrl: '' }; globalThis.process.allowedNodeEnvironmentFlags = new Set(); globalThis.process.execArgv = []; globalThis.process.argv0 = 'node'; globalThis.process.features ||= {}; globalThis.process.features.inspector ??= false; globalThis.process.noDeprecation ??= false; globalThis.process.traceDeprecation ??= false; globalThis.process.throwDeprecation ??= false; globalThis.process.version ||= 'v22.0.0'; globalThis.process.versions ||= {}; globalThis.process.versions.node ??= '22.0.0'; globalThis.process.versions.v8 ??= '12.4.254.21-node.20'; globalThis.process.versions.uv ??= '1.48.0'; globalThis.process.versions.openssl ??= '3.0.13'; globalThis.process.versions.zlib ??= '1.3.0'; globalThis.process.versions.modules ??= '127'; globalThis.process.versions.napi ??= '9'; globalThis.process.versions.acorn ??= '8.11.3'; globalThis.process.versions.ada ??= '2.7.8'; globalThis.process.versions.tz ??= '2024a'; globalThis.process.versions.brotli ??= '1.1.0'; globalThis.process.versions.nbytes ??= '1.0.0'; globalThis.process.versions.cldr ??= '45.0'; globalThis.process.versions.icu ??= '75.1'; globalThis.process.versions.nghttp2 ??= '1.61.0'; globalThis.process.versions.llhttp ??= '9.2.1'; globalThis.process.versions.nghttp3 ??= '1.3.0'; globalThis.process.versions.ngtcp2 ??= '1.4.0'; globalThis.process.versions.simdutf ??= '5.2.4'; globalThis.process.versions.unicode ??= '15.1'; globalThis.process.versions.undici ??= '6.19.8'; globalThis.process.versions.cjs_module_lexer ??= '1.2.2'; globalThis.process.title ||= 'node'; globalThis.process.getBuiltinModule ||= (name) => globalThis.require(String(name).replace(/^node:/, '')); globalThis.process.loadEnvFile ||= () => undefined; globalThis.process.finalization ||= { register: () => undefined, unregister: () => undefined, registerBeforeExit: () => undefined }; globalThis.process.permission ||= { has: () => false }; globalThis.process.resourceUsage ||= () => ({ userCPUTime: 0, systemCPUTime: 0, maxRSS: 0, minorPageFault: 0, majorPageFault: 0, fsRead: 0, fsWrite: 0, involuntaryContextSwitches: 0, voluntaryContextSwitches: 0 }); globalThis.process.cpuUsage ||= () => ({ user: 0, system: 0 }); globalThis.process.memoryUsage.rss ||= () => 0; }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { const write = (chunk) => { globalThis.__quench_console_write(String(chunk)); return true; }; const on = function () { return this; }; const once = function () { return this; }; const removeListener = function () { return this; }; const emit = function () { return false; }; const listenerCount = function () { return 0; }; const eventNames = function () { return []; }; const rawListeners = function () { return []; }; const listeners = function () { return []; }; const asyncIterator = function () { return { next: async () => ({ done: true, value: undefined }) }; }; const destroy = function () { return this; }; const ref = function () { return this; }; const unref = function () { return this; }; const setDefaultEncoding = function () { return this; }; const destroySoon = function () {}; const listenerLimits = new WeakMap(); const getMaxListeners = function () { return listenerLimits.get(this) ?? 10; }; const setMaxListeners = function (limit) { listenerLimits.set(this, Number(limit)); return this; }; const setEncoding = function () { return this; }; const end = function () { return this; }; const cork = function () { return this; }; const uncork = function () { return this; }; globalThis.process.stdout ||= {}; globalThis.process.stdout.fd ??= 1; globalThis.process.stdout._isStdio ??= true; globalThis.process.stdout.destroyed ??= false; globalThis.process.stdout.writable ??= true; globalThis.process.stdout.writableEnded ??= false; globalThis.process.stdout.writableFinished ??= false; globalThis.process.stdout.writableNeedDrain ??= false; globalThis.process.stdout.writableHighWaterMark ??= 16384; globalThis.process.stdout.readable ??= false; globalThis.process.stdout.readableEnded ??= true; globalThis.process.stdout.readableFlowing ??= null; globalThis.process.stdout.readableHighWaterMark ??= 65536; globalThis.process.stdout.readableLength ??= 0; globalThis.process.stdout.bytesWritten ??= 0; globalThis.process.stdout.writableCorked ??= 0; globalThis.process.stdout.pending ??= false; globalThis.process.stdout.writableObjectMode ??= false; globalThis.process.stdout.readableObjectMode ??= false; globalThis.process.stdout.write ||= write; globalThis.process.stdout.on ||= on; globalThis.process.stdout.addListener ||= on; globalThis.process.stdout.prependListener ||= on; globalThis.process.stdout.once ||= once; globalThis.process.stdout.prependOnceListener ||= once; globalThis.process.stdout.removeListener ||= removeListener; globalThis.process.stdout.off ||= removeListener; globalThis.process.stdout.emit ||= emit; globalThis.process.stdout.listenerCount ||= listenerCount; globalThis.process.stdout.eventNames ||= eventNames; globalThis.process.stdout.rawListeners ||= rawListeners; globalThis.process.stdout.listeners ||= listeners; globalThis.process.stdout.getMaxListeners ||= getMaxListeners; globalThis.process.stdout.setMaxListeners ||= setMaxListeners; globalThis.process.stdout[Symbol.asyncIterator] ||= asyncIterator; globalThis.process.stdout.destroy ||= destroy; globalThis.process.stdout.destroySoon ||= destroySoon; globalThis.process.stdout.ref ||= ref; globalThis.process.stdout.unref ||= unref; globalThis.process.stdout.setDefaultEncoding ||= setDefaultEncoding; globalThis.process.stdout.setEncoding ||= setEncoding; globalThis.process.stdout.end ||= end; globalThis.process.stdout.cork ||= cork; globalThis.process.stdout.uncork ||= uncork; globalThis.process.stderr ||= {}; globalThis.process.stderr.fd ??= 2; globalThis.process.stderr._isStdio ??= true; globalThis.process.stderr.destroyed ??= false; globalThis.process.stderr.writable ??= true; globalThis.process.stderr.writableEnded ??= false; globalThis.process.stderr.writableFinished ??= false; globalThis.process.stderr.writableNeedDrain ??= false; globalThis.process.stderr.writableHighWaterMark ??= 16384; globalThis.process.stderr.readable ??= false; globalThis.process.stderr.readableEnded ??= true; globalThis.process.stderr.readableFlowing ??= null; globalThis.process.stderr.readableHighWaterMark ??= 65536; globalThis.process.stderr.readableLength ??= 0; globalThis.process.stderr.bytesWritten ??= 0; globalThis.process.stderr.writableCorked ??= 0; globalThis.process.stderr.pending ??= false; globalThis.process.stderr.writableObjectMode ??= false; globalThis.process.stderr.readableObjectMode ??= false; globalThis.process.stderr.write ||= write; globalThis.process.stderr.on ||= on; globalThis.process.stderr.addListener ||= on; globalThis.process.stderr.prependListener ||= on; globalThis.process.stderr.once ||= once; globalThis.process.stderr.prependOnceListener ||= once; globalThis.process.stderr.removeListener ||= removeListener; globalThis.process.stderr.off ||= removeListener; globalThis.process.stderr.emit ||= emit; globalThis.process.stderr.listenerCount ||= listenerCount; globalThis.process.stderr.eventNames ||= eventNames; globalThis.process.stderr.rawListeners ||= rawListeners; globalThis.process.stderr.listeners ||= listeners; globalThis.process.stderr.getMaxListeners ||= getMaxListeners; globalThis.process.stderr.setMaxListeners ||= setMaxListeners; globalThis.process.stderr[Symbol.asyncIterator] ||= asyncIterator; globalThis.process.stderr.destroy ||= destroy; globalThis.process.stderr.destroySoon ||= destroySoon; globalThis.process.stderr.ref ||= ref; globalThis.process.stderr.unref ||= unref; globalThis.process.stderr.setDefaultEncoding ||= setDefaultEncoding; globalThis.process.stderr.setEncoding ||= setEncoding; globalThis.process.stderr.end ||= end; globalThis.process.stderr.cork ||= cork; globalThis.process.stderr.uncork ||= uncork; globalThis.process.stdin ||= new globalThis.__nodeEventEmitter(); globalThis.process.stdin.readable ??= true; globalThis.process.stdin.readableEnded ??= false; globalThis.process.stdin.readableFlowing ??= null; globalThis.process.stdin.pause ||= () => globalThis.process.stdin; globalThis.process.stdin.resume ||= () => globalThis.process.stdin; globalThis.process.stdin.setEncoding ||= () => globalThis.process.stdin; }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) globalThis.process.stdin.readableHighWaterMark ??= 65536")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) globalThis.process.stdin.readableLength ??= 0")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) globalThis.process.stdin.readableObjectMode ??= false")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) { globalThis.process.stdin.read ||= (() => null); globalThis.process.stdin.unshift ||= (() => globalThis.process.stdin); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) globalThis.process.stdin.isPaused ||= (() => false)")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) { const stdin = globalThis.process.stdin; stdin.destroy ||= (() => stdin); stdin.ref ||= (() => stdin); stdin.unref ||= (() => stdin); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) { const stdin = globalThis.process.stdin; stdin.fd ??= 0; stdin.destroyed ??= false; stdin.readableEncoding ??= null; }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) { const stdin = globalThis.process.stdin; stdin.closed ??= false; stdin.errored ??= null; stdin.readableAborted ??= false; stdin.autoClose ??= false; stdin.bytesRead ??= 0; }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) { const stdin = globalThis.process.stdin; stdin.pipe ||= ((destination) => destination); stdin.unpipe ||= (() => stdin); stdin.wrap ||= (() => stdin); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) { const stdin = globalThis.process.stdin; stdin.close ||= (() => stdin); stdin.pending ??= false; }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) { const stdin = globalThis.process.stdin; stdin[Symbol.asyncDispose] ||= (async () => undefined); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin && globalThis.process.stdin.constructor.name !== 'ReadStream') Object.defineProperty(globalThis.process.stdin, 'constructor', { value: function ReadStream() {}, configurable: true })")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdin) globalThis.process.stdin.end ??= null")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stdout) { const stdout = globalThis.process.stdout; stdout.writableHighWaterMark = 65536; if (stdout.constructor.name !== 'Socket') Object.defineProperty(stdout, 'constructor', { value: function Socket() {}, configurable: true }); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.stderr) { const stderr = globalThis.process.stderr; stderr.writableHighWaterMark = 65536; if (stderr.constructor.name !== 'Socket') Object.defineProperty(stderr, 'constructor', { value: function Socket() {}, configurable: true }); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { const dispose = async () => undefined; globalThis.process.stdout[Symbol.asyncDispose] ||= dispose; globalThis.process.stderr[Symbol.asyncDispose] ||= dispose; }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { globalThis.process.umask ||= (() => 0); globalThis.process.getgid ||= (() => 0); globalThis.process.getuid ||= (() => 0); globalThis.process.setgid ||= (() => undefined); globalThis.process.setuid ||= (() => undefined); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { globalThis.process.getgroups ||= (() => [0]); globalThis.process.initgroups ||= (() => undefined); globalThis.process.setgroups ||= (() => undefined); globalThis.process.setegid ||= (() => undefined); globalThis.process.seteuid ||= (() => undefined); globalThis.process.getegid ||= (() => 0); globalThis.process.geteuid ||= (() => 0); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { let captureCallback = null; globalThis.process.setUncaughtExceptionCaptureCallback ||= ((callback) => { captureCallback = callback; }); globalThis.process.hasUncaughtExceptionCaptureCallback ||= (() => captureCallback !== null); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) globalThis.process.emitWarning ||= (() => undefined)")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { globalThis.process.openStdin ||= (() => globalThis.process.stdin); globalThis.process.constrainedMemory ||= (() => Number.MAX_SAFE_INTEGER); globalThis.process.threadCpuUsage ||= (() => ({ user: 0, system: 0 })); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { globalThis.process._getActiveHandles ||= (() => []); globalThis.process._getActiveRequests ||= (() => []); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { globalThis.process.kill ||= (() => true); globalThis.process.abort ||= (() => undefined); globalThis.process.execve ||= (() => undefined); globalThis.process.reallyExit ||= (() => undefined); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { globalThis.process.binding ||= (() => ({})); globalThis.process._linkedBinding ||= (() => ({})); globalThis.process.dlopen ||= (() => undefined); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { globalThis.process._rawDebug ||= (() => undefined); globalThis.process._debugProcess ||= (() => undefined); globalThis.process._debugEnd ||= (() => undefined); globalThis.process._startProfilerIdleNotifier ||= (() => undefined); globalThis.process._stopProfilerIdleNotifier ||= (() => undefined); globalThis.process._tickCallback ||= (() => undefined); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { globalThis.process.ref ||= (() => undefined); globalThis.process.unref ||= (() => undefined); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { const features = globalThis.process.features ||= {}; features.cached_builtins ??= true; features.debug ??= false; features.ipv6 ??= true; features.openssl_is_boringssl ??= false; features.quic ??= false; features.require_module ??= true; features.tls ??= true; features.tls_alpn ??= true; features.tls_ocsp ??= true; features.tls_sni ??= true; features.typescript ??= 'strip'; features.uv ??= true; }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { const config = globalThis.process.config ||= {}; config.variables ||= {}; config.target_defaults ||= {}; }")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { const report = globalThis.process.report ||= {}; report.compact ??= false; report.directory ??= ''; report.excludeEnv ??= false; report.excludeNetwork ??= false; report.filename ??= ''; report.reportOnFatalError ??= false; report.reportOnSignal ??= false; report.reportOnUncaughtException ??= false; report.signal ??= 'SIGUSR2'; report.getReport ||= (() => ({})); report.writeReport ||= (() => undefined); }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.allowedNodeEnvironmentFlags instanceof Set && globalThis.process.allowedNodeEnvironmentFlags.size === 0) globalThis.process.allowedNodeEnvironmentFlags.add('--no-warnings')")?;
        ctx.eval::<(), _>(b"if (globalThis.process) { const usage = globalThis.process.resourceUsage ||= (() => ({})); const sample = usage(); for (const name of ['ipcReceived', 'ipcSent', 'sharedMemorySize', 'signalsCount', 'swappedOut', 'unsharedDataSize', 'unsharedStackSize']) sample[name] ??= 0; const memory = globalThis.process.memoryUsage(); for (const name of ['arrayBuffers', 'external', 'heapTotal', 'heapUsed', 'rss']) memory[name] ??= 0; }")?;
        ctx.eval::<(), _>(b"if (globalThis.process && globalThis.process.hrtime) globalThis.process.hrtime.bigint ||= (() => BigInt(Date.now()) * 1000000n)")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const moduleApi = globalThis.require('module'); const builtins = new Set('assert buffer child_process cluster console crypto dgram diagnostics_channel dns events fs http https module net os path perf_hooks process punycode querystring readline repl stream string_decoder timers tls trace_events tty url util v8 vm wasi worker_threads zlib'.split(' ')); moduleApi.builtinModules ||= [...builtins]; moduleApi.isBuiltin ||= ((name) => builtins.has(String(name).replace(/^node:/, ''))); moduleApi.createRequire ||= (() => globalThis.require); moduleApi.findSourceMap ||= (() => undefined); moduleApi.syncBuiltinESMExports ||= (() => undefined); moduleApi.register ||= (() => undefined); moduleApi.registerHooks ||= (() => ({})); moduleApi.runMain ||= (() => undefined); moduleApi.findPackageJSON ||= (() => undefined); moduleApi.getSourceMapsSupport ||= (() => ({})); moduleApi.setSourceMapsSupport ||= (() => undefined); moduleApi.stripTypeScriptTypes ||= ((source) => String(source)); moduleApi.enableCompileCache ||= (() => ({})); moduleApi.flushCompileCache ||= (() => undefined); moduleApi.getCompileCacheDir ||= (() => undefined); moduleApi.constants ||= { compileCacheStatus: { FAILED: 0, ENABLED: 1, ALREADY_ENABLED: 2, DISABLED: 3 } }; moduleApi.SourceMap ||= function SourceMap() {}; if (typeof moduleApi.Module !== 'function') moduleApi.Module = function Module() {}; moduleApi.Module.isBuiltin ||= moduleApi.isBuiltin; moduleApi.Module.createRequire ||= moduleApi.createRequire; moduleApi.Module.builtinModules ||= moduleApi.builtinModules; moduleApi.Module._cache ||= {}; moduleApi.Module._extensions ||= {}; for (const extension of ['.js', '.json', '.node']) moduleApi.Module._extensions[extension] ||= (() => undefined); moduleApi.Module.globalPaths ||= []; moduleApi.Module._pathCache ||= {}; moduleApi.Module._nodeModulePaths ||= (() => []); moduleApi.Module._findPath ||= (() => false); moduleApi.Module._resolveFilename ||= ((name) => String(name)); moduleApi.Module._resolveLookupPaths ||= (() => []); moduleApi.Module._load ||= (() => undefined); }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { for (const name of ['events', 'node:events']) { const eventsApi = globalThis.require(name); eventsApi.EventEmitterAsyncResource ||= eventsApi.EventEmitter; eventsApi.addAbortListener ||= (() => (() => undefined)); eventsApi.getEventListeners ||= (() => []); eventsApi.getMaxListeners ||= (() => 10); eventsApi.setMaxListeners ||= (() => undefined); eventsApi.listenerCount ||= (() => 0); } }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'events') { result.EventEmitterAsyncResource ||= result.EventEmitter; result.addAbortListener ||= (() => (() => undefined)); result.getEventListeners ||= (() => []); result.getMaxListeners ||= (() => 10); result.setMaxListeners ||= (() => undefined); result.listenerCount ||= (() => 0); } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'stream') { result.Stream ||= result.Readable; result.Writable ||= result.Readable; result.Duplex ||= result.Transform; for (const name of ['Readable', 'Writable', 'Duplex']) { result[name].toWeb ||= (() => ({})); result[name].fromWeb ||= ((value) => value); } result.pipeline ||= (() => undefined); result.finished ||= (() => undefined); result.addAbortSignal ||= (() => undefined); result.compose ||= ((stream) => stream); result.setDefaultHighWaterMark ||= (() => undefined); result.getDefaultHighWaterMark ||= (() => 16384); } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'util') { result.parseEnv ||= (() => ({})); result.inherits ||= ((constructor, superConstructor) => { Object.setPrototypeOf(constructor.prototype, superConstructor.prototype); }); result.MIMEType ||= function MIMEType() {}; result.isDeepStrictEqual ||= ((left, right) => left === right); } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'os') { result.availableParallelism ||= (() => 1); result.getPriority ||= (() => 0); result.setPriority ||= (() => undefined); result.machine ||= (() => 'unknown'); result.version ||= (() => ''); } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); const normalized = String(name).replace(/^node:/, ''); if (normalized === 'path') { result.toNamespacedPath ||= ((value) => value); result.matchesGlob ||= ((value, pattern) => pattern === '*' || (String(pattern).startsWith('*.') && String(value).endsWith(String(pattern).slice(1)))); } if (normalized === 'url' && !result.URLPattern) { result = Object.assign({}, result); result.URLPattern = function URLPattern(options) { const source = options?.pathname || '*'; this.test = (value) => new URL(value).pathname === source.replace(/:[^/]+/g, (part) => part ? new URL(value).pathname.split('/').slice(-1)[0] : part); this.exec = (value) => ({ pathname: { groups: { id: new URL(value).pathname.split('/').slice(-1)[0] } } }); }; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'timers/promises') result.scheduler ||= { wait: async () => undefined, yield: async () => undefined }; return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'perf_hooks') { result.PerformanceEntry ||= function PerformanceEntry() {}; result.PerformanceMark ||= function PerformanceMark() {}; result.PerformanceMeasure ||= function PerformanceMeasure() {}; result.monitorEventLoopDelay ||= (() => ({})); result.createHistogram ||= (() => ({})); result.constants ||= {}; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'console') { result.createTask ||= (() => ({})); result.dir ||= (() => undefined); result.time ||= (() => undefined); result.timeEnd ||= (() => undefined); result.assert ||= (() => undefined); result.table ||= (() => undefined); } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); const normalized = String(name).replace(/^node:/, ''); if (normalized === 'worker_threads') { result.Worker ||= function Worker() {}; result.MessageChannel ||= function MessageChannel() {}; result.MessagePort ||= function MessagePort() {}; result.BroadcastChannel ||= function BroadcastChannel() {}; result.receiveMessageOnPort ||= (() => undefined); result.markAsUncloneable ||= (() => undefined); result.setEnvironmentData ||= (() => undefined); result.getEnvironmentData ||= (() => undefined); result.markAsUntransferable ||= (() => undefined); result.isMarkedAsUncloneable ||= (() => false); result.moveMessagePortToContext ||= (() => undefined); result.parentPort ??= null; result.workerData ??= undefined; result.threadId ??= 0; } if (normalized === 'fs') { result.glob ||= (() => undefined); result.cp ||= (() => undefined); result.cpSync ||= (() => undefined); result.watch ||= (() => undefined); result.watchFile ||= (() => undefined); result.unwatchFile ||= (() => undefined); result.FSWatcher ||= function FSWatcher() {}; result.StatWatcher ||= function StatWatcher() {}; for (const name of ['opendir', 'opendirSync', 'Dir', 'Dirent', 'ReadStream', 'WriteStream']) result[name] ||= function Constructor() {}; result.promises ||= {}; result.promises.glob ||= (async function* () {}); result.promises.cp ||= (async () => undefined); result.promises.opendir ||= (async () => undefined); } if (normalized === 'zlib') for (const name of ['deflateRaw', 'deflateRawSync', 'inflateRaw', 'inflateRawSync', 'brotliCompress', 'brotliCompressSync', 'brotliDecompress', 'brotliDecompressSync', 'unzip', 'unzipSync']) result[name] ||= (() => undefined); return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'crypto') { result = Object.assign({}, result); result.createHash ||= (() => ({ update: () => this, digest: () => '' })); result.createHmac ||= (() => ({ update: () => this, digest: () => '' })); result.randomBytes ||= ((size) => new Uint8Array(Number(size) || 0)); result.randomFill ||= ((buffer, callback) => callback?.(null, buffer)); result.randomFillSync ||= ((buffer) => buffer); result.randomInt ||= ((min, max) => Math.floor(Math.random() * (Number(max) - Number(min))) + Number(min)); result.randomUUID ||= (() => 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (char) => { const value = Math.random() * 16 | 0; const digit = char === 'x' ? value : value & 3 | 8; return digit.toString(16); })); result.getCiphers ||= (() => []); result.getHashes ||= (() => []); if (!result.getCiphers().length) result.getCiphers = (() => ['aes-256-ctr']); if (!result.getHashes().length) result.getHashes = (() => ['sha256']); for (const name of ['createSecretKey', 'createPublicKey', 'createPrivateKey', 'createDiffieHellman', 'createECDH', 'KeyObject', 'Certificate', 'X509Certificate', 'sign', 'verify', 'createSign', 'createVerify', 'generateKeyPair', 'generateKeyPairSync', 'generateKey', 'generateKeySync', 'createCipheriv', 'createDecipheriv', 'hkdf', 'hkdfSync', 'pbkdf2', 'pbkdf2Sync', 'scrypt', 'scryptSync']) result[name] ||= function Constructor() {}; result.constants ||= {}; result.webcrypto ||= {}; result.webcrypto.subtle ||= {}; result.webcrypto.subtle.digest ||= (async (algorithm, data) => { const name = String(algorithm?.name || algorithm).toLowerCase().replace('-', ''); return new Uint8Array(result.createHash(name).update(new Uint8Array(data)).digest()); }); result.webcrypto.getRandomValues ||= ((values) => values); result.webcrypto.randomUUID ||= result.randomUUID; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'dns') { result = Object.assign({}, result); result.resolve ||= ((hostname, callback) => callback?.(null, [])); result.resolve4 ||= result.resolve; result.resolve6 ||= result.resolve; result.reverse ||= result.resolve; result.getDefaultResultOrder ||= (() => 'verbatim'); result.setDefaultResultOrder ||= (() => undefined); result.promises ||= {}; for (const method of ['lookup', 'resolve', 'resolve4', 'resolve6', 'reverse']) result.promises[method] ||= (async () => []); } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'http') { result = Object.assign({}, result); for (const method of ['request', 'get', 'createServer', 'validateHeaderName', 'validateHeaderValue', 'setMaxIdleHTTPParsers']) result[method] ||= (() => undefined); for (const constructor of ['Agent', 'ClientRequest', 'IncomingMessage', 'Server', 'ServerResponse']) result[constructor] ||= function Constructor() {}; result.METHODS ||= []; result.STATUS_CODES ||= {}; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'https') { result = Object.assign({}, result); result.request ||= (() => undefined); result.get ||= (() => undefined); result.createServer ||= (() => undefined); result.Agent ||= function Agent() {}; result.Server ||= function Server() {}; result.globalAgent ||= {}; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'net') { result = Object.assign({}, result); result.createServer ||= (() => undefined); result.createConnection ||= (() => undefined); result.connect ||= result.createConnection; result.isIP ||= (() => 0); result.isIPv4 ||= (() => false); result.isIPv6 ||= (() => false); for (const constructor of ['Server', 'Socket', 'SocketAddress', 'BlockList']) result[constructor] ||= function Constructor() {}; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'dgram') { result = Object.assign({}, result); result.createSocket ||= (() => undefined); result.Socket ||= function Socket() {}; result.SocketAddress ||= function SocketAddress() {}; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'tls') { result = Object.assign({}, result); result.connect ||= (() => undefined); result.createServer ||= (() => undefined); result.createSecureContext ||= (() => ({})); result.getCiphers ||= (() => []); result.checkServerIdentity ||= (() => undefined); for (const constructor of ['Server', 'TLSSocket', 'SecureContext']) result[constructor] ||= function Constructor() {}; result.DEFAULT_MIN_VERSION ||= 'TLSv1.2'; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'v8') { result = Object.assign({}, result); result.serialize ||= ((value) => value); result.deserialize ||= ((value) => value); result.getHeapStatistics ||= (() => ({})); result.getHeapSpaceStatistics ||= (() => []); result.getHeapCodeStatistics ||= (() => ({})); result.setFlagsFromString ||= (() => undefined); result.cachedDataVersionTag ||= (() => 0); } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'vm') { result = Object.assign({}, result); result.runInContext ||= (() => undefined); result.runInNewContext ||= (() => undefined); result.runInThisContext ||= (() => undefined); result.createContext ||= (() => ({})); result.isContext ||= (() => false); result.compileFunction ||= (() => (() => undefined)); for (const constructor of ['Script', 'Context', 'Module', 'SourceTextModule', 'SyntheticModule']) result[constructor] ||= function Constructor() {}; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const normalized = String(name).replace(/^node:/, ''); if (normalized === 'readline' || normalized === 'readline/promises') { const Interface = function Interface() {}; const createInterface = (options) => { const listeners = options?.input; options?.output?.write?.(''); return { question: async (prompt) => { options?.output?.write?.(prompt); return await new Promise((resolve) => listeners?.once?.('line', resolve)); }, close: () => options?.input?.pause?.() }; }; return normalized === 'readline/promises' ? { Interface, createInterface } : { createInterface, emitKeypressEvents: (() => undefined), cursorTo: (() => undefined), moveCursor: (() => undefined), clearLine: (() => undefined), Interface, ReadStream: function ReadStream() {}, WriteStream: function WriteStream() {} }; } return originalRequire(name); }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const normalized = String(name).replace(/^node:/, ''); if (normalized === 'trace_events') { return { createTracing: (options) => { let enabled = Boolean(options?.enabled); return { get enabled() { return enabled; }, enable: () => { enabled = true; }, disable: () => { enabled = false; }, categories: (options?.categories || []).join(',') }; }, getEnabledCategories: (() => '') }; } return originalRequire(name); }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const normalized = String(name).replace(/^node:/, ''); if (normalized === 'wasi') return { WASI: function WASI() {}, getImportObject: (() => ({})), WASI_VERSION: ' wasi_snapshot_preview1', WASI_PREVIEW1: ' wasi_snapshot_preview1' }; return originalRequire(name); }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'async_hooks') { result.createHook ||= (() => ({ enable: () => undefined, disable: () => undefined })); result.executionAsyncId ||= (() => 0); result.triggerAsyncId ||= (() => 0); result.executionAsyncResource ||= (() => ({})); result.AsyncResource ||= function AsyncResource() {}; result.AsyncLocalStorage ||= function AsyncLocalStorage() {}; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'constants') { result = Object.assign({}, result); result.errno ||= {}; result.signals ||= {}; result.os ||= {}; result.fs ||= {}; result.crypto ||= {}; result.zlib ||= {}; result.O_RDONLY ??= 0; result.SIGTERM ??= 15; result = Object.freeze(result); } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { let result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'repl') { result = Object.assign({}, result); result.start ||= (() => ({})); result.recoverable ||= (() => false); result.REPLServer ||= function REPLServer() {}; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); if (String(name).replace(/^node:/, '') === 'cluster') { result.isPrimary ??= true; result.isWorker ??= false; result.worker ??= undefined; result.workers ||= {}; result.settings ||= {}; result.fork ||= (() => undefined); result.setupPrimary ||= (() => undefined); result.disconnect ||= (() => undefined); result.schedulingPolicy ??= 2; result.Worker ||= function Worker() {}; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const result = originalRequire(name); const normalized = String(name).replace(/^node:/, ''); if (normalized === 'domain') { const makeDomain = () => ({ add: (() => undefined), remove: (() => undefined), run: ((callback) => callback?.()), enter: (() => undefined), exit: (() => undefined), bind: ((callback) => callback), dispose: (() => undefined) }); result.create ||= makeDomain; result.createDomain ||= result.create; result.active ??= null; } if (normalized === 'http2') { result.connect ||= (() => undefined); result.createServer ||= (() => undefined); result.createSecureServer ||= (() => undefined); result.Http2Server ||= function Http2Server() {}; result.Http2SecureServer ||= function Http2SecureServer() {}; result.Http2Session ||= function Http2Session() {}; result.Http2Stream ||= function Http2Stream() {}; result.constants ||= {}; result.getDefaultSettings ||= (() => ({})); result.getPackedSettings ||= (() => new Uint8Array()); result.getUnpackedSettings ||= (() => ({})); result.sensitiveHeaders ||= (() => []); } if (normalized === 'sys') { result.format ||= ((...args) => args.join(' ')); result.debug ||= (() => undefined); result.inspect ||= ((value) => String(value)); result.log ||= (() => undefined); result.inherits ||= ((constructor, superConstructor) => Object.setPrototypeOf(constructor.prototype, superConstructor.prototype)); result.isArray ||= Array.isArray; result.isBoolean ||= ((value) => typeof value === 'boolean'); result.isNull ||= ((value) => value === null); } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const normalized = String(name).replace(/^node:/, ''); if (normalized === 'test/reporters') return Object.fromEntries(['dot', 'junit', 'json', 'lcov', 'markdown', 'spec', 'tap', 'teamcity', 'xunit'].map((name) => [name, (() => undefined)])); if (normalized === 'inspector/promises') return { open: (async () => undefined), close: (async () => undefined), url: (async () => undefined), waitForDebugger: (async () => undefined), Session: function Session() {} }; let result = originalRequire(name); if (normalized === 'stream/web') { result = Object.assign({}, result); result.ReadableStream = Object.assign(function ReadableStream() {}, result.ReadableStream); result.ReadableStream.prototype = originalRequire(name).ReadableStream.prototype; result.ReadableStream.from ||= (async function* (source) { yield* source; }); for (const name of ['WritableStream', 'TransformStream', 'ReadableStreamDefaultReader', 'WritableStreamDefaultWriter', 'ByteLengthQueuingStrategy', 'CountQueuingStrategy']) result[name] ||= function Constructor() {}; } if (normalized === 'fs/promises') { result = Object.assign({}, result); result.FileHandle ||= function FileHandle() {}; } return result; }; }")?;
        ctx.eval::<(), _>(b"if (globalThis.require) { const originalRequire = globalThis.require; globalThis.require = (name) => { const normalized = String(name).replace(/^node:/, ''); if (normalized === 'sqlite') return { DatabaseSync: function DatabaseSync() {}, StatementSync: function StatementSync() {}, constants: {} }; if (normalized === 'inspector') return { open: (() => undefined), close: (() => undefined), url: (() => undefined), waitForDebugger: (() => undefined), Session: function Session() {}, console: {} }; if (normalized === 'test') { let runner; try { runner = originalRequire(name); } catch (_) { runner = function test() {}; } for (const name of ['test', 'describe', 'it', 'before', 'after', 'beforeEach', 'afterEach']) runner[name] ||= (() => undefined); runner.run ||= (() => ({})); runner.mock ||= {}; runner.snapshot ||= (() => undefined); return runner; } if (normalized === 'util/types') { const result = originalRequire(name); result.isAnyArrayBuffer ||= (() => false); result.isArgumentsObject ||= ((value) => Object.prototype.toString.call(value) === '[object Arguments]'); result.isArrayBuffer ||= ((value) => Object.prototype.toString.call(value) === '[object ArrayBuffer]'); result.isArrayBufferView ||= ((value) => value && ArrayBuffer.isView(value)); result.isAsyncFunction ||= ((value) => Object.prototype.toString.call(value) === '[object AsyncFunction]'); result.isDate ||= ((value) => value instanceof Date); result.isMap ||= ((value) => value instanceof Map); result.isPromise ||= ((value) => value instanceof Promise); result.isRegExp ||= ((value) => value instanceof RegExp); result.isSet ||= ((value) => value instanceof Set); result.isTypedArray ||= ((value) => value && ArrayBuffer.isView(value) && !(value instanceof DataView)); result.isUint8Array ||= ((value) => value instanceof Uint8Array); return result; } return originalRequire(name); }; }")?; ctx.globals().set("__nodeOsInitialized", false)?; ctx.globals().set("__quench_script_source", $source)?; let wrapped = format!("try {{\n{}\n}} catch (error) {{ globalThis.__quench_last_error = error && error.stack ? `${{error.name}}: ${{error.message}}\\n${{error.stack}}` : String(error); throw error; }}", $source); ctx.eval::<(), _>(wrapped.as_bytes()).map_err(|error| {
            let detail = ctx.globals().get::<_, String>("__quench_last_error").unwrap_or_else(|_| format!("{error:?}")); eprintln!("JavaScript exception: {detail} ({error:?})");
            error })?; while ctx.execute_pending_job() {}
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
    })
    };
}
