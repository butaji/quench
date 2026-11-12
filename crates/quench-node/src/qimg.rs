//! The explicit ahead-of-time pair: `quench-node compile app.js -o app.qimg` runs the entry
//! once with recording on and writes the entry path plus the images of every program that run
//! compiled; `quench-node app.qimg [args]` stages those images and runs the stored entry.
//! Nothing is looked up by name or loaded implicitly. A program whose fingerprint differs from
//! its image simply gets no image (`AheadImages::is_of`) and runs as usual.

use quench_runtime::{AheadImages, Runtime};
use std::{cell::RefCell, process::ExitCode};

use crate::host::NodeHost;

/// Extension that routes a command line to [`run`].
pub const EXTENSION: &str = ".qimg";
const LENGTH_BYTES: usize = std::mem::size_of::<u32>();
/// Ends an executable `compile` wrote: the runtime, the image file, its length, then this.
const TRAILER_MAGIC: &[u8; 8] = b"QUENCHX\0";
const PAYLOAD_LENGTH_BYTES: usize = std::mem::size_of::<u64>();
const TRAILER_BYTES: usize = PAYLOAD_LENGTH_BYTES + TRAILER_MAGIC.len();
const EXECUTABLE_MODE: u32 = 0o755;

thread_local! { static STAGED: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) }; }

/// The payload length a trailer states, or `None` where it is not one.
fn payload_length(trailer: &[u8]) -> Option<u64> {
    let (length, magic) = trailer.split_at_checked(PAYLOAD_LENGTH_BYTES)?;
    (magic == TRAILER_MAGIC).then(|| u64::from_le_bytes(length.try_into().expect("length bytes")))
}

/// The image file appended to the running executable, if `compile` made it; only its end is read.
fn embedded() -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut exe = std::fs::File::open(std::env::current_exe().ok()?).ok()?;
    let mut trailer = [0; TRAILER_BYTES];
    exe.seek(SeekFrom::End(-(TRAILER_BYTES as i64))).ok()?;
    exe.read_exact(&mut trailer).ok()?;
    let length = payload_length(&trailer)?;
    exe.seek(SeekFrom::End(-((TRAILER_BYTES as u64 + length) as i64))).ok()?;
    let mut payload = vec![0; usize::try_from(length).ok()?];
    exe.read_exact(&mut payload).ok()?;
    Some(payload)
}

/// This executable without anything `compile` appended to it.
fn bare_runtime() -> Result<Vec<u8>, String> {
    let mut exe = std::fs::read(std::env::current_exe().map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let payload = exe.len().checked_sub(TRAILER_BYTES).and_then(|at| payload_length(&exe[at..]));
    if let Some(length) = payload { exe.truncate(exe.len() - TRAILER_BYTES - length as usize); }
    Ok(exe)
}

/// Writes `file` as `out`: an image file for a `.qimg` name, otherwise an executable (this
/// runtime with the image file appended, which `route` finds at start).
fn write_output(out: &str, file: Vec<u8>) -> Result<(), String> {
    let written = if out.ends_with(EXTENSION) { file } else {
        let mut exe = bare_runtime()?;
        let length = file.len() as u64;
        exe.extend(file);
        exe.extend(length.to_le_bytes());
        exe.extend(TRAILER_MAGIC);
        exe
    };
    std::fs::write(out, written).map_err(|error| format!("{out}: {error}"))?;
    #[cfg(unix)]
    if !out.ends_with(EXTENSION) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(out, std::fs::Permissions::from_mode(EXECUTABLE_MODE)).map_err(|error| format!("{out}: {error}"))?;
    }
    Ok(())
}

/// `compile <entry.js> -o <out> [entry args]`: `<out>.qimg` is an image file (run with
/// `quench-node <out>.qimg`), any other name an executable.
pub fn compile(arguments: Vec<String>) -> Result<ExitCode, String> {
    let (entry, rest) = arguments.split_first().ok_or("usage: quench-node compile <entry.js> -o <out>")?;
    let flag = rest.iter().position(|argument| argument == "-o").ok_or("compile needs -o <out>")?;
    let (before, [_, out, after @ ..]) = rest.split_at(flag) else { return Err("-o needs a file name".into()); };
    let entry_args = before.iter().chain(after).cloned();
    let entry = std::fs::canonicalize(entry).map_err(|error| format!("{entry}: {error}"))?;
    let entry = entry.to_string_lossy().into_owned();
    eprintln!("quench-node compile: running {entry} to record its images; the program executes with its side effects");
    AheadImages::start_recording();
    let status = crate::shared_run::run_shared_cli(std::iter::once(entry.clone()).chain(entry_args));
    let images = AheadImages::finish_recording();
    let mut file = (entry.len() as u32).to_le_bytes().to_vec();
    file.extend_from_slice(entry.as_bytes());
    file.extend(AheadImages::bundle(&images));
    write_output(out, file)?;
    status
}

/// `<x>.qimg [args]`: stage the image file's programs, then run its entry.
pub fn run(image: &str, arguments: impl IntoIterator<Item = String>) -> Result<ExitCode, String> {
    let file = std::fs::read(image).map_err(|error| format!("{image}: {error}"))?;
    run_file(image, &file, arguments)
}

/// Runs the entry an image file names, with its programs' images staged.
fn run_file(image: &str, file: &[u8], arguments: impl IntoIterator<Item = String>) -> Result<ExitCode, String> {
    let (length, body) = file.split_at_checked(LENGTH_BYTES).ok_or("truncated image file")?;
    let length = u32::from_le_bytes(length.try_into().map_err(|_| "truncated image file")?) as usize;
    let (entry, bundle) = body.split_at_checked(length).ok_or("truncated image file")?;
    match AheadImages::unbundle(bundle) {
        Ok(_) => STAGED.with(|staged| *staged.borrow_mut() = bundle.to_vec()),
        Err(error) => eprintln!("quench-node: {image}: {error}; running without images"),
    }
    let entry = String::from_utf8(entry.to_vec()).map_err(|error| error.to_string())?;
    crate::shared_run::run_shared_cli(std::iter::once(entry).chain(arguments))
}

/// Hands the staged images, if any, to a freshly built runtime.
pub(crate) fn install(runtime: &mut Runtime<NodeHost>) {
    STAGED.with(|staged| {
        let staged = staged.borrow();
        if staged.is_empty() { return; }
        for images in AheadImages::unbundle(&staged).unwrap_or_default() { runtime.load_ahead_images(images); }
    });
}

/// The command line's AOT command, if it is one: `compile ...` or `<x>.qimg ...`.
pub fn route(arguments: &[String]) -> Option<Result<ExitCode, String>> {
    if let Some(payload) = embedded() { return Some(run_file("this executable", &payload, arguments.iter().cloned())); }
    match arguments {
        [command, rest @ ..] if command == "compile" && !cfg!(feature = "aot") => Some(compile(rest.to_vec())),
        [image, rest @ ..] if image.ends_with(EXTENSION) => Some(run(image, rest.iter().cloned())),
        _ => None,
    }
}
