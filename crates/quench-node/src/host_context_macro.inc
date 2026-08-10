#[macro_export]
macro_rules! quench_eval_post_bootstrap { ($ctx:expr; $($name:literal),* $(,)?) => { $(
    let source = include_str!(concat!("../polyfills/post-bootstrap/", $name, ".js"));
    if !source.is_empty() { $ctx.eval::<(), _>(source.as_bytes())?; }
)* }; }

#[macro_export]
macro_rules! run_host_context {
    ($context:expr, $source:expr) => {
    $context.with(|ctx| -> rquickjs::Result<()> {
        ctx.globals().set("__quench_tcp_bind", Func::from($crate::host_context::quench_tcp_bind))?;
        ctx.globals().set("__quench_tcp_bound_port", Func::from($crate::host_context::quench_tcp_bound_port))?;
        ctx.globals().set("__quench_tcp_local_port", Func::from($crate::host_context::quench_tcp_local_port))?;
        ctx.globals().set("__quench_tcp_peer_port", Func::from($crate::host_context::quench_tcp_peer_port))?;
        ctx.globals().set("__quench_tcp_accept", Func::from($crate::host_context::quench_tcp_accept))?;
        ctx.globals().set("__quench_tcp_connect", Func::from($crate::host_context::quench_tcp_connect))?;
        ctx.globals().set("__quench_tcp_read", Func::from($crate::host_context::quench_tcp_read))?;
        ctx.globals().set("__quench_tcp_readable", Func::from($crate::host_context::quench_tcp_readable))?;
        ctx.globals().set("__quench_tcp_write", Func::from($crate::host_context::quench_tcp_write))?;
        ctx.globals().set("__quench_tcp_shutdown", Func::from($crate::host_context::quench_tcp_shutdown))?;
        ctx.globals().set("__quench_tcp_close", Func::from($crate::host_context::quench_tcp_close))?;
        ctx.globals().set("__quench_udp_socket", Func::from($crate::host_context::quench_udp_socket))?;
        ctx.globals().set("__quench_udp_send", Func::from($crate::host_context::quench_udp_send))?;
        ctx.globals().set("__quench_udp_recv", Func::from($crate::host_context::quench_udp_recv))?;
        ctx.globals().set("__quench_udp_close", Func::from($crate::host_context::quench_tcp_close))?;
        ctx.globals().set("__quench_dns_lookup", Func::from($crate::host_context::quench_dns_lookup))?;
        ctx.globals().set("__quench_dns_reverse", Func::from($crate::host_context::quench_dns_reverse))?;
        ctx.globals().set("__quench_fs_exists", Func::from(|path: String| fs::metadata(path).is_ok()))?; ctx.globals().set("__quench_cwd", std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).to_string_lossy().into_owned())?; ctx.globals().set("__quench_cwd_get", Func::from(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).to_string_lossy().into_owned()))?; ctx.globals().set("__quench_chdir", Func::from(|path: String| -> rquickjs::Result<()> { std::env::set_current_dir(path).map_err(|_| rquickjs::Error::new_from_js("process", "chdir failed")) }))?; ctx.globals().set("__quench_umask", Func::from(|mask: Option<u32>| -> u32 { #[cfg(unix)] unsafe { let current = libc::umask(mask.unwrap_or(0o022) as libc::mode_t); if mask.is_none() { libc::umask(current); } current as u32 } #[cfg(not(unix))] { mask.unwrap_or(0o022) } }))?;
        ctx.globals().set("__quench_env_get", Func::from(|key: String| std::env::var(key).ok()))?;
        ctx.globals().set("__quench_env_set", Func::from(|key: String, value: String| { std::env::set_var(key, value); }))?;
        ctx.globals().set("__quench_env_delete", Func::from(|key: String| { std::env::remove_var(key); }))?;
        ctx.globals().set("__quench_console_write", Func::from(|line: String| { println!("{line}"); }))?;
        ctx.globals().set("__quench_now_ns", Func::from(|| SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos().to_string()))?;
        ctx.globals().set("__quench_sleep_ms", Func::from(|milliseconds: u64| { std::thread::sleep(std::time::Duration::from_millis(milliseconds.min(60_000))); }))?;
        ctx.globals().set("__quench_pid", std::process::id())?;
        ctx.globals().set("__quench_exec_path", std::env::current_exe().unwrap_or_else(|_| PathBuf::from("quench-node")).to_string_lossy().into_owned())?;
        ctx.globals().set("__filename", ctx.globals().get::<_, String>("__quench_script_filename").unwrap_or_else(|_| std::env::current_exe().unwrap_or_else(|_| PathBuf::from("quench-node")).to_string_lossy().into_owned()))?;
        let script_argv = ctx.globals().get::<_, String>("__quench_script_filename").ok();
        let quench_argv = match script_argv {
            Some(filename) => {
                let mut argv = vec![
                    std::env::current_exe()
                        .unwrap_or_else(|_| PathBuf::from("quench-node"))
                        .to_string_lossy()
                        .into_owned(),
                    filename,
                ];
                if let Ok(script_args) = ctx.globals().get::<_, Vec<String>>("__quench_script_args") {
                    argv.extend(script_args);
                    argv.extend(env::args().skip(1).filter(|arg| {
                        arg.starts_with("--experimental-") ||
                            arg.starts_with("--network-family-autoselection")
                    }));
                } else {
                    argv.extend(
                        env::args()
                            .skip(1)
                            .filter(|arg| {
                                arg.starts_with("--experimental-")
                                    || arg.starts_with("--network-family-autoselection")
                            }),
                    );
                }
                argv
            }
            None => env::args().collect::<Vec<String>>(),
        };
        ctx.globals().set("__quench_argv", quench_argv)?;
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
        ctx.globals().set("__quench_getuid", { #[cfg(unix)] { Some(unsafe { libc::getuid() as u32 }) } #[cfg(not(unix))] { None::<u32> } })?;
        ctx.globals().set("__quench_geteuid", { #[cfg(unix)] { Some(unsafe { libc::geteuid() as u32 }) } #[cfg(not(unix))] { None::<u32> } })?;
        ctx.globals().set("__quench_getgid", { #[cfg(unix)] { Some(unsafe { libc::getgid() as u32 }) } #[cfg(not(unix))] { None::<u32> } })?;
        ctx.globals().set("__quench_getegid", { #[cfg(unix)] { Some(unsafe { libc::getegid() as u32 }) } #[cfg(not(unix))] { None::<u32> } })?;
        ctx.globals().set("__quench_digest_bytes", Func::from(|algorithm: String, value: Vec<u8>| -> rquickjs::Result<Vec<u8>> { match algorithm.as_str() { "sha1" => Ok(Sha1::digest(value).to_vec()), "sha224" => Ok(Sha224::digest(value).to_vec()), "sha256" => Ok(Sha256::digest(value).to_vec()), "sha384" => Ok(Sha384::digest(value).to_vec()), "sha512" => Ok(Sha512::digest(value).to_vec()), "md5" => Ok(Md5::digest(value).to_vec()), _ => Err(rquickjs::Error::new_from_js("crypto", "unsupported digest")) } }))?;
        ctx.globals().set("__quench_shake_bytes", Func::from(|algorithm: String, value: Vec<u8>, length: u32| -> Vec<u8> { let mut output = vec![0u8; length as usize]; match algorithm.as_str() { "shake128" => { let mut hasher = Shake128::default(); hasher.update(&value); sha3::digest::XofReader::read(&mut hasher.finalize_xof(), &mut output); } _ => { let mut hasher = Shake256::default(); hasher.update(&value); sha3::digest::XofReader::read(&mut hasher.finalize_xof(), &mut output); } } output }))?;
        ctx.globals().set("__quench_pbkdf2_bytes", Func::from(|password: Vec<u8>, salt: Vec<u8>, iterations: u32, keylen: u32| -> Vec<u8> { let mut output = vec![0u8; keylen as usize]; for (block, chunk) in output.chunks_mut(32).enumerate() { let mut input = salt.clone(); input.extend_from_slice(&(block as u32 + 1).to_be_bytes()); let mut mac = Hmac::<Sha256>::new_from_slice(&password).expect("valid HMAC key"); Mac::update(&mut mac, &input); let mut previous = mac.finalize().into_bytes().to_vec(); let mut derived = previous.clone(); for _ in 1..iterations { let mut mac = Hmac::<Sha256>::new_from_slice(&password).expect("valid HMAC key"); Mac::update(&mut mac, &previous); previous = mac.finalize().into_bytes().to_vec(); for (left, right) in derived.iter_mut().zip(&previous) { *left ^= *right; } } chunk.copy_from_slice(&derived[..chunk.len()]); } output }))?;
        ctx.globals().set("__quench_random_uuid", Func::from(|| { let mut bytes = [0u8; 16]; rand::thread_rng().fill_bytes(&mut bytes); bytes[6] = (bytes[6] & 0x0f) | 0x40; bytes[8] = (bytes[8] & 0x3f) | 0x80; format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]) }))?;
        ctx.globals().set("__quench_random_bytes", Func::from(|size: u64| -> Vec<u8> { let mut bytes = vec![0u8; size.min(16 * 1024 * 1024) as usize]; rand::thread_rng().fill_bytes(&mut bytes); bytes }))?;
        ctx.globals().set("__quench_zlib_deflate", Func::from(|value: Vec<u8>| -> rquickjs::Result<Vec<u8>> { use flate2::write::DeflateEncoder; use flate2::Compression; use std::io::Write; let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default()); let write_result = encoder.write_all(&value); if let Err(e) = write_result { return Err(rquickjs::Error::new_from_js_message("zlib", "deflate failed", e.to_string())); } match encoder.finish() { Ok(v) => Ok(v), Err(e) => Err(rquickjs::Error::new_from_js_message("zlib", "deflate finish failed", e.to_string())), } }))?;
        ctx.globals().set("__quench_zlib_inflate", Func::from(|value: Vec<u8>| -> rquickjs::Result<Vec<u8>> { use flate2::read::DeflateDecoder; use std::io::Read; let mut decoder = DeflateDecoder::new(&value[..]); let mut out = Vec::new(); match decoder.read_to_end(&mut out) { Ok(_) => Ok(out), Err(e) => Err(rquickjs::Error::new_from_js_message("zlib", "inflate failed", e.to_string())), } }))?;
        ctx.globals().set("__quench_zlib_gzip", Func::from(|value: Vec<u8>, level: rquickjs::function::Opt<u32>| -> rquickjs::Result<Vec<u8>> { use flate2::write::GzEncoder; use flate2::Compression; use std::io::Write; let compression = level.0.map(Compression::new).unwrap_or_default(); let mut encoder = GzEncoder::new(Vec::new(), compression); let write_result = encoder.write_all(&value); if let Err(e) = write_result { return Err(rquickjs::Error::new_from_js_message("zlib", "gzip failed", e.to_string())); } match encoder.finish() { Ok(v) => Ok(v), Err(e) => Err(rquickjs::Error::new_from_js_message("zlib", "gzip finish failed", e.to_string())), } }))?;
        ctx.globals().set("__quench_zlib_gunzip", Func::from(|value: Vec<u8>| -> rquickjs::Result<Vec<u8>> { use flate2::read::GzDecoder; use std::io::Read; let mut decoder = GzDecoder::new(&value[..]); let mut out = Vec::new(); match decoder.read_to_end(&mut out) { Ok(_) => Ok(out), Err(e) => Err(rquickjs::Error::new_from_js_message("zlib", "gunzip failed", e.to_string())), } }))?;
        ctx.globals().set("__quench_fs_mkdtemp", Func::from(|prefix: String| -> rquickjs::Result<String> { let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos() as usize; let sequence = MKDTEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed); for attempt in 0..10_000 { let suffix = (stamp.wrapping_add(sequence).wrapping_add(attempt)) % 1_000_000; let path = PathBuf::from(format!("{prefix}{suffix:06}")); if fs::create_dir(&path).is_ok() { return Ok(path.to_string_lossy().into_owned()); } } Err(rquickjs::Error::new_from_js("fs", "mkdtemp failed")) }))?;
        ctx.globals().set("__quench_fs_read_file", Func::from(|path: String| -> rquickjs::Result<String> { fs::read_to_string(path).map_err(|_| rquickjs::Error::new_from_js("fs", "readFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_read_text", Func::from(|path: String| -> rquickjs::Result<String> { fs::read_to_string(path).map_err(|_| rquickjs::Error::new_from_js("fs", "readFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_write_file", Func::from(|path: String, data: String| -> rquickjs::Result<()> { fs::write(path, data).map_err(|_| rquickjs::Error::new_from_js("fs", "writeFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_truncate", Func::from(|path: String, length: u64| -> rquickjs::Result<()> { fs::OpenOptions::new().write(true).open(path).and_then(|file| file.set_len(length)).map_err(|_| rquickjs::Error::new_from_js("fs", "truncate failed")) }))?;
        ctx.globals().set("__quench_fs_read_bytes", Func::from(|path: String| -> rquickjs::Result<Vec<u8>> { fs::read(path).map_err(|_| rquickjs::Error::new_from_js("fs", "readFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_read_range_bytes", Func::from(|path: String, position: u64, length: u64| -> rquickjs::Result<Vec<u8>> { let mut file = fs::File::open(path).map_err(|_| rquickjs::Error::new_from_js("fs", "read failed"))?; file.seek(SeekFrom::Start(position)).map_err(|_| rquickjs::Error::new_from_js("fs", "read failed"))?; let mut bytes = vec![0; length.min(1024 * 1024) as usize]; let count = file.read(&mut bytes).map_err(|_| rquickjs::Error::new_from_js("fs", "read failed"))?; bytes.truncate(count); Ok(bytes) }))?;
        ctx.globals().set("__quench_fs_write_bytes", Func::from(|path: String, bytes: Vec<u8>| -> rquickjs::Result<()> { fs::write(path, bytes).map_err(|_| rquickjs::Error::new_from_js("fs", "writeFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_native_open", Func::from(|path: String, flags: String| -> rquickjs::Result<i32> { #[cfg(unix)] { let bits = match flags.as_str() { "r" => libc::O_RDONLY, "r+" => libc::O_RDWR, "w" => libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC, "w+" => libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC, "a" => libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND, "a+" => libc::O_RDWR | libc::O_CREAT | libc::O_APPEND, _ => return Err(rquickjs::Error::new_from_js("fs", "openSync failed")) }; let path = std::ffi::CString::new(path).map_err(|_| rquickjs::Error::new_from_js("fs", "openSync failed"))?; let fd = unsafe { libc::open(path.as_ptr(), bits, 0o666) }; if fd < 0 { Err(rquickjs::Error::new_from_js("fs", "openSync failed")) } else { Ok(fd) } } #[cfg(not(unix))] let _ = (path, flags); #[cfg(not(unix))] Err(rquickjs::Error::new_from_js("fs", "openSync failed")) }))?;
        ctx.globals().set("__quench_fs_native_realpath", Func::from(|path: String| -> rquickjs::Result<String> { std::fs::canonicalize(&path).map(|value| value.to_string_lossy().into_owned()).map_err(|_| rquickjs::Error::new_from_js("fs", "realpath failed")) }))?;
        ctx.globals().set("__quench_fs_native_readlink", Func::from(|path: String| -> rquickjs::Result<String> { std::fs::read_link(&path).map(|value| value.to_string_lossy().into_owned()).map_err(|_| rquickjs::Error::new_from_js("fs", "readlink failed")) }))?;
        ctx.globals().set("__quench_fs_native_read_all", Func::from(|fd: i32| -> rquickjs::Result<Vec<u8>> { #[cfg(unix)] { let mut stat = unsafe { std::mem::zeroed::<libc::stat>() }; if unsafe { libc::fstat(fd, &mut stat) } < 0 { return Err(rquickjs::Error::new_from_js("fs", "readFileSync failed")); } let mut output = vec![0u8; stat.st_size.max(0) as usize]; let count = unsafe { libc::pread(fd, output.as_mut_ptr().cast(), output.len(), 0) }; if count < 0 { return Err(rquickjs::Error::new_from_js("fs", "readFileSync failed")); } output.truncate(count as usize); return Ok(output); } #[cfg(not(unix))] let _ = fd; #[cfg(not(unix))] Err(rquickjs::Error::new_from_js("fs", "readFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_native_write", Func::from(|fd: i32, bytes: Vec<u8>| -> rquickjs::Result<()> { #[cfg(unix)] { let count = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) }; if count < 0 || count as usize != bytes.len() { return Err(rquickjs::Error::new_from_js("fs", "writeFileSync failed")); } return Ok(()); } #[cfg(not(unix))] let _ = (fd, bytes); #[cfg(not(unix))] Err(rquickjs::Error::new_from_js("fs", "writeFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_native_read_at", Func::from(|fd: i32, position: u64, length: u32| -> rquickjs::Result<Vec<u8>> { #[cfg(unix)] { let mut bytes = vec![0u8; length as usize]; let count = unsafe { libc::pread(fd, bytes.as_mut_ptr().cast(), bytes.len(), position as libc::off_t) }; if count < 0 { return Err(rquickjs::Error::new_from_js("fs", "read failed")); } bytes.truncate(count as usize); return Ok(bytes); } #[cfg(not(unix))] let _ = (fd, position, length); #[cfg(not(unix))] Err(rquickjs::Error::new_from_js("fs", "read failed")) }))?;
        ctx.globals().set("__quench_fs_native_write_at", Func::from(|fd: i32, position: u64, bytes: Vec<u8>| -> rquickjs::Result<u32> { #[cfg(unix)] { let count = unsafe { libc::pwrite(fd, bytes.as_ptr().cast(), bytes.len(), position as libc::off_t) }; if count < 0 { return Err(rquickjs::Error::new_from_js("fs", "write failed")); } return Ok(count as u32); } #[cfg(not(unix))] let _ = (fd, position, bytes); #[cfg(not(unix))] Err(rquickjs::Error::new_from_js("fs", "write failed")) }))?;
        ctx.globals().set("__quench_fs_native_truncate", Func::from(|fd: i32, length: u64| -> rquickjs::Result<()> { #[cfg(unix)] { let result = unsafe { libc::ftruncate(fd, length as libc::off_t) }; if result < 0 { return Err(rquickjs::Error::new_from_js("fs", "truncate failed")); } return Ok(()); } #[cfg(not(unix))] let _ = (fd, length); #[cfg(not(unix))] Err(rquickjs::Error::new_from_js("fs", "truncate failed")) }))?;
        ctx.globals().set("__quench_fs_native_close", Func::from(|fd: i32| -> rquickjs::Result<()> { #[cfg(unix)] { if unsafe { libc::close(fd) } != 0 { return Err(rquickjs::Error::new_from_js("fs", "closeSync failed")); } return Ok(()); } #[cfg(not(unix))] let _ = fd; #[cfg(not(unix))] Err(rquickjs::Error::new_from_js("fs", "closeSync failed")) }))?;
        ctx.globals().set("__quench_fs_open", Func::from(|path: String, flags: String| -> rquickjs::Result<u32> { use std::fs::OpenOptions; let mut options = OpenOptions::new(); if flags.starts_with('r') { options.read(true); } else { options.create(true).write(true); if flags.starts_with('w') { options.truncate(true); } } options.open(path).map(|_| 1).map_err(|_| rquickjs::Error::new_from_js("fs", "openSync failed")) }))?;
        ctx.globals().set("__quench_fs_mkdir", Func::from(|path: String| -> rquickjs::Result<()> { fs::create_dir_all(path).map_err(|_| rquickjs::Error::new_from_js("fs", "mkdirSync failed")) }))?;
        ctx.globals().set("__quench_fs_readdir", Func::from(|path: String| -> rquickjs::Result<Vec<String>> { fs::read_dir(path).map_err(|_| rquickjs::Error::new_from_js("fs", "readdirSync failed"))?.map(|entry| entry.map(|item| item.file_name().to_string_lossy().into_owned()).map_err(|_| rquickjs::Error::new_from_js("fs", "readdirSync failed"))).collect() }))?;
        ctx.globals().set("__quench_fs_remove_dir", Func::from(|path: String| -> rquickjs::Result<()> { fs::remove_dir_all(&path).map_err(|error| { let detail = if error.kind() == std::io::ErrorKind::PermissionDenied { format!("EACCES: permission denied, rmdir '{path}'") } else { format!("rmdirSync failed: {error}") }; rquickjs::Error::new_from_js_message("fs", "rmdirSync failed", detail) }) }))?;
        ctx.globals().set("__quench_fs_kind", Func::from(|path: String| -> rquickjs::Result<String> { let metadata = fs::metadata(path).map_err(|_| rquickjs::Error::new_from_js("fs", "statSync failed"))?; Ok(if metadata.is_file() { "file".into() } else if metadata.is_dir() { "directory".into() } else { "other".into() }) }))?;
        ctx.globals().set("__quench_fs_link_kind", Func::from(|path: String| -> rquickjs::Result<String> { let metadata = fs::symlink_metadata(path).map_err(|_| rquickjs::Error::new_from_js("fs", "lstatSync failed"))?; Ok(if metadata.file_type().is_symlink() { "symlink".into() } else if metadata.is_file() { "file".into() } else if metadata.is_dir() { "directory".into() } else { "other".into() }) }))?;
        ctx.globals().set("__quench_fs_rename", Func::from(|from: String, to: String| -> rquickjs::Result<()> { fs::rename(from, to).map_err(|_| rquickjs::Error::new_from_js("fs", "renameSync failed")) }))?;
        ctx.globals().set("__quench_fs_unlink", Func::from(|path: String| -> rquickjs::Result<()> { fs::remove_file(path).map_err(|_| rquickjs::Error::new_from_js("fs", "unlinkSync failed")) }))?;
        ctx.globals().set("__quench_fs_copy", Func::from(|from: String, to: String| -> rquickjs::Result<()> { fs::copy(from, to).map(|_| ()).map_err(|_| rquickjs::Error::new_from_js("fs", "copyFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_append", Func::from(|path: String, data: String| -> rquickjs::Result<()> { use std::io::Write; let mut file = fs::OpenOptions::new().create(true).append(true).open(path).map_err(|_| rquickjs::Error::new_from_js("fs", "appendFileSync failed"))?; file.write_all(data.as_bytes()).map_err(|_| rquickjs::Error::new_from_js("fs", "appendFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_append_bytes", Func::from(|path: String, data: Vec<u8>| -> rquickjs::Result<()> { use std::io::Write; let mut file = fs::OpenOptions::new().create(true).append(true).open(path).map_err(|_| rquickjs::Error::new_from_js("fs", "appendFileSync failed"))?; file.write_all(&data).map_err(|_| rquickjs::Error::new_from_js("fs", "appendFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_append_typed", Func::from(|path: String, data: rquickjs::TypedArray<'_, u8>| -> rquickjs::Result<()> { use std::io::Write; let bytes = data.as_bytes().ok_or_else(|| rquickjs::Error::new_from_js("fs", "appendFileSync failed"))?; let mut file = fs::OpenOptions::new().create(true).append(true).open(path).map_err(|_| rquickjs::Error::new_from_js("fs", "appendFileSync failed"))?; file.write_all(bytes).map_err(|_| rquickjs::Error::new_from_js("fs", "appendFileSync failed")) }))?;
        ctx.globals().set("__quench_fs_realpath", Func::from(|path: String| -> rquickjs::Result<String> { fs::canonicalize(path).map(|value| value.to_string_lossy().into_owned()).map_err(|_| rquickjs::Error::new_from_js("fs", "realpathSync failed")) }))?;
        ctx.globals().set("__quench_fs_chmod", Func::from(|path: String, mode: u32| -> rquickjs::Result<()> { #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let mut permissions = fs::metadata(&path).map_err(|_| rquickjs::Error::new_from_js("fs", "chmodSync failed"))?.permissions(); permissions.set_mode(mode); fs::set_permissions(path, permissions).map_err(|_| rquickjs::Error::new_from_js("fs", "chmodSync failed"))?; } #[cfg(not(unix))] let _ = (path, mode); Ok(()) }))?;
        ctx.globals().set("__quench_fs_symlink", Func::from(|target: String, link: String| -> rquickjs::Result<()> { std::os::unix::fs::symlink(target, link).map_err(|_| rquickjs::Error::new_from_js("fs", "symlinkSync failed")) }))?;
        ctx.globals().set("__quench_fs_link", Func::from(|source: String, destination: String| -> rquickjs::Result<()> { fs::hard_link(source, destination).map_err(|_| rquickjs::Error::new_from_js("fs", "link failed")) }))?;
        ctx.globals().set("__quench_fs_readlink", Func::from(|path: String| -> rquickjs::Result<String> { std::fs::read_link(path).map(|value| value.to_string_lossy().into_owned()).map_err(|_| rquickjs::Error::new_from_js("fs", "readlinkSync failed")) }))?;
        ctx.globals().set("__quench_host_timer_scheduler", true)?;
        ctx.eval::<(), _>(BOOTSTRAP_SOURCE.as_bytes()).map_err(|error| { eprintln!("Bootstrap JavaScript exception: {error:?}"); error })?;
        let cli_title = $source.lines().find_map(|line| { line.trim().strip_prefix("// Flags:").and_then(|flags| flags.split_whitespace().find_map(|flag| flag.strip_prefix("--title="))) });
        ctx.globals().set("__quench_cli_title", cli_title)?;
        quench_eval_post_bootstrap!(ctx; "process-surface-00", "process-surface-01", "process-surface-02", "process-surface-03", "process-surface-04", "process-surface-05", "process-surface-06", "process-surface-07", "process-surface-08", "process-surface-09", "process-surface-10", "process-surface-11", "process-surface-12", "process-surface-13", "process-surface-14", "process-surface-15", "process-surface-16", "process-surface-17", "process-surface-18", "process-surface-19", "process-surface-20", "process-surface-21", "process-surface-22", "process-surface-23", "process-surface-24", "process-surface-25", "process-surface-26", "process-surface-27", "process-surface-28", "process-surface-29", "process-surface-30", "process-surface-31", "process-surface-32", "process-surface-33", "module-surface-00", "module-surface-01", "module-surface-02", "module-surface-03", "module-surface-03-tail", "module-surface-03-tail-02", "module-surface-04", "module-surface-05", "module-surface-06", "module-surface-06-tail", "module-surface-07", "module-surface-08", "module-surface-09", "module-surface-10", "module-surface-11", "module-surface-11-tail", "module-surface-11-final", "module-surface-12", "module-surface-12-tail", "module-surface-13", "module-surface-13-tail", "module-surface-14", "module-surface-14-tail", "module-surface-15", "module-surface-15-tail", "module-surface-16", "module-surface-17", "module-surface-18", "module-surface-19", "module-surface-20", "module-surface-21", "module-surface-22", "module-surface-23", "module-surface-24", "module-surface-25", "module-surface-26", "module-surface-27", "module-surface-28", "module-surface-29", "module-local-loader", "module-surface-final-00");
        ctx.eval::<(), _>(include_str!("../polyfills/post-bootstrap/module-surface-final-01.js").as_bytes())?; ctx.eval::<(), _>(include_str!("../polyfills/post-bootstrap/module-surface-final-01-tail-02.js").as_bytes())?; ctx.eval::<(), _>(include_str!("../polyfills/post-bootstrap/module-surface-final-01-tail.js").as_bytes())?; ctx.eval::<(), _>(include_str!("../polyfills/post-bootstrap/global-surface.js").as_bytes())?; ctx.globals().set("__nodeOsInitialized", false)?; ctx.globals().set("__quench_script_source", $source)?; let filename = ctx.globals().get::<_, String>("__quench_script_filename").unwrap_or_else(|_| "quench-entry.js".into()); let evaluation = if filename.ends_with(".mjs") { let meta_url = format!("file://{}", filename); rquickjs::Module::declare(ctx.clone(), filename, $source.as_bytes()).and_then(|module| { let meta = module.meta()?; meta.set("url", meta_url)?; let (_, promise) = module.eval()?; ctx.globals().set("__quench_entry_promise", promise)?; ctx.eval::<(), _>(b"Promise.resolve(globalThis.__quench_entry_promise).then(() => { globalThis.__quench_entry_done = true; globalThis.__quench_work_generation = (globalThis.__quench_work_generation || 0) + 1; }, error => { globalThis.__quench_entry_error = String(error); globalThis.__quench_entry_done = true; globalThis.__quench_work_generation = (globalThis.__quench_work_generation || 0) + 1; });") }) } else { let wrapped = format!("try {{\nconst __quenchEntryModule = globalThis.module;\n(function (exports, require, module, __filename, __dirname) {{\n{}\n}}).call(__quenchEntryModule.exports, __quenchEntryModule.exports, globalThis.require, __quenchEntryModule, globalThis.__filename, globalThis.__dirname);\n}} catch (error) {{ globalThis.__quench_last_error = error && error.stack ? `${{error.name}}: ${{error.message}}\\n${{error.stack}}` : String(error); throw error; }}", $source); ctx.eval::<(), _>(wrapped.as_bytes()) }; if let Err(error) = evaluation {
            let forced_exit = ctx.globals().get::<_, bool>("__quench_force_exit").unwrap_or(false);
            if !forced_exit {
                let detail = ctx.globals().get::<_, String>("__quench_last_error").unwrap_or_else(|_| error.to_string());
                eprintln!("JavaScript exception: {detail} ({error:?})");
                return Err(error);
            }
        }
        ctx.eval::<(), _>(b"if (globalThis.__quench_test_promises?.length) Promise.allSettled(globalThis.__quench_test_promises).then(() => { globalThis.__quench_tests_settled = true; });")?;
        ctx.eval::<(), _>(b"if (typeof globalThis.__quench_io_poll === 'function') globalThis.__quench_io_poll();")?;
        while ctx.execute_pending_job() {
        }
        ctx.eval::<(), _>(b"if (typeof globalThis.__quench_timer_poll === 'function') globalThis.__quench_timer_poll();")?;
        while ctx.execute_pending_job() {}
        if let Ok(detail) = ctx.globals().get::<_, String>("__quench_async_error") {
            if !detail.is_empty() {
                eprintln!("Asynchronous JavaScript exception: {detail}");
                return Err(rquickjs::Error::new_from_js("async", "exception"));
            }
        }
        ctx.eval::<(), _>(include_str!("../polyfills/post-bootstrap/global-surface.js").as_bytes()).map_err(|error| { eprintln!("Final global surface failure: {error:?}"); error })?;
        if !ctx.globals().get::<_, bool>("__quench_force_exit").unwrap_or(false) {
            loop {
                let pending_timer = ctx.eval::<i64, _>(b"typeof globalThis.__quench_timer_next_delay === 'function' ? globalThis.__quench_timer_next_delay() : -1")?;
                if pending_timer >= 0 {
                    std::thread::sleep(std::time::Duration::from_millis(pending_timer.min(60_000) as u64));
                    ctx.eval::<(), _>(b"if (typeof globalThis.__quench_io_poll === 'function') globalThis.__quench_io_poll(); if (typeof globalThis.__quench_timer_poll === 'function') globalThis.__quench_timer_poll();")?;
                    while ctx.execute_pending_job() {
                        ctx.eval::<(), _>(b"if (typeof globalThis.__quench_timer_poll === 'function') globalThis.__quench_timer_poll();")?;
                    }
                    continue;
                }
                let work_before = ctx.eval::<i64, _>(b"globalThis.__quench_work_generation || 0")?;
                let before_exit = ctx.eval::<(), _>(b"try { if (typeof process?.emit === 'function') process.emit('beforeExit', process.exitCode || 0); } catch (error) { globalThis.__quench_exit_error = error && error.stack ? `${error.name}: ${error.message}\\n${error.stack}` : String(error); throw error; }");
                if let Err(error) = before_exit {
                    if !ctx.globals().get::<_, bool>("__quench_force_exit").unwrap_or(false) {
                        let detail = ctx.globals().get::<_, String>("__quench_exit_error").unwrap_or_else(|_| format!("{error:?}"));
                        eprintln!("Process beforeExit handler failure: {detail}");
                        ctx.globals().set("__quench_force_exit", true)?;
                    }
                    break;
                }
                ctx.eval::<(), _>(b"try { if (typeof globalThis.__quench_io_poll === 'function') globalThis.__quench_io_poll(); if (typeof globalThis.__quench_timer_poll === 'function') globalThis.__quench_timer_poll(); } catch (error) { globalThis.__quench_exit_error = error && error.stack ? `${error.name}: ${error.message}\\n${error.stack}` : String(error); throw error; }")
                    .map_err(|error| {
                        let detail = ctx.globals().get::<_, String>("__quench_exit_error").unwrap_or_else(|_| format!("{error:?}"));
                        eprintln!("Host I/O poll failure: {detail}");
                        error
                    })?;
                let mut ran_job = false;
                while ctx.execute_pending_job() {
                    ran_job = true;
                    ctx.eval::<(), _>(b"if (typeof globalThis.__quench_timer_poll === 'function') globalThis.__quench_timer_poll();")?;
                }
                if ran_job {
                    let work_after = ctx.eval::<i64, _>(b"globalThis.__quench_work_generation || 0")?;
                    if work_after > work_before { continue; }
                    break;
                }
                if !ran_job {
                    let wait_ms = ctx.eval::<i64, _>(b"typeof globalThis.__quench_timer_next_delay === 'function' ? globalThis.__quench_timer_next_delay() : -1")?;
                    if wait_ms < 0 {
                        let work_after = ctx.eval::<i64, _>(b"globalThis.__quench_work_generation || 0")?;
                        if work_after > work_before { continue; }
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(wait_ms.min(60_000) as u64));
                }
            }
        }
        ctx.eval::<(), _>(b"try { Object.defineProperty(globalThis, '__quench_emitting_exit', { configurable: true, value: true }); if (typeof process?.emit === 'function') process.emit('exit', process.exitCode || 0); } catch (error) { globalThis.__quench_exit_error = error && error.stack ? `${error.name}: ${error.message}\\n${error.stack}` : String(error); throw error; }")
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
