//! The JIT image file: `quench compile app.js -o app.qimg` runs the entry once with recording on
//! and writes the entry path plus the images of every program that ran compiled; `quench app.qimg
//! [args]` stages those images and runs the stored entry. An executable is not made here: a
//! non-`.qimg` `-o` is the linked image `aot::cli` builds, and nothing here embeds a runtime.
//! Nothing is looked up by name or loaded implicitly. A program whose fingerprint differs from
//! its image simply gets no image (`AheadImages::is_of`) and runs as usual.

use quench_runtime::{AheadImages, Runtime};
use quench_layers::engine::AheadImagesOps1;
use std::{process::ExitCode, sync::Mutex};

use crate::host::NodeHost;

/// Extension that routes a command line to [`run`], and the only output name [`compile`] accepts.
pub const EXTENSION: &str = ".qimg";
const LENGTH_BYTES: usize = std::mem::size_of::<u32>();

/// The staged image file, process-wide: the entry runs on a worker thread (`run_shared_cli`), not the one that read the file.
static STAGED: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Refuses an output name that is not an image file name, before anything is recorded or written.
fn check_name(out: &str) -> Result<(), String> {
    if out.ends_with(EXTENSION) { Ok(()) } else { Err(format!("{out}: qimg writes only {EXTENSION} image files")) }
}

/// Writes `file` as the image file `out`.
fn write_output(out: &str, file: Vec<u8>) -> Result<(), String> {
    check_name(out)?;
    std::fs::write(out, file).map_err(|error| format!("{out}: {error}"))
}

/// `compile <entry.js> -o <out.qimg> [entry args]`: writes the image file, run with `quench <out>.qimg`.
pub fn compile(arguments: Vec<String>) -> Result<ExitCode, String> {
    let (entry, rest) = arguments.split_first().ok_or("usage: quench compile <entry.js> -o <out.qimg>")?;
    let flag = rest.iter().position(|argument| argument == "-o").ok_or("compile needs -o <out.qimg>")?;
    let (before, [_, out, after @ ..]) = rest.split_at(flag) else { return Err("-o needs a file name".into()); };
    check_name(out)?;
    let entry_args = before.iter().chain(after).cloned();
    let entry = std::fs::canonicalize(entry).map_err(|error| format!("{entry}: {error}"))?;
    let entry = entry.to_string_lossy().into_owned();
    eprintln!("quench compile: running {entry} to record its images; the program executes with its side effects");
    AheadImages::start_recording();
    let status = crate::shared_run::run_shared_cli(std::iter::once(entry.clone()).chain(entry_args));
    let images = AheadImages::finish_recording();
    let mut file = (entry.len() as u32).to_le_bytes().to_vec();
    file.extend_from_slice(entry.as_bytes());
    file.extend(AheadImages::bundle(&images));
    write_output(out, file)?;
    status
}

/// `[options] <x>.qimg [args]`: stage the image file's programs, then run its entry under `options`
/// (the mode flags, as `--b-jit`, which precede a script).
pub fn run(options: &[String], image: &str, arguments: impl IntoIterator<Item = String>) -> Result<ExitCode, String> {
    let file = std::fs::read(image).map_err(|error| format!("{image}: {error}"))?;
    run_file(options, image, &file, arguments)
}

/// Runs the entry an image file names, with its programs' images staged.
fn run_file(options: &[String], image: &str, file: &[u8], arguments: impl IntoIterator<Item = String>) -> Result<ExitCode, String> {
    let (length, body) = file.split_at_checked(LENGTH_BYTES).ok_or("truncated image file")?;
    let length = u32::from_le_bytes(length.try_into().map_err(|_| "truncated image file")?) as usize;
    let (entry, bundle) = body.split_at_checked(length).ok_or("truncated image file")?;
    match AheadImages::unbundle(bundle) {
        Ok(_) => *STAGED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = bundle.to_vec(),
        Err(error) => eprintln!("quench: {image}: {error}; running without images"),
    }
    let entry = String::from_utf8(entry.to_vec()).map_err(|error| error.to_string())?;
    crate::shared_run::run_shared_cli(options.iter().cloned().chain(std::iter::once(entry)).chain(arguments))
}

/// Hands the staged images, if any, to a freshly built runtime.
pub(crate) fn install(runtime: &mut Runtime<NodeHost>) {
    let staged = STAGED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if staged.is_empty() { return; }
    for images in AheadImages::unbundle(&staged).unwrap_or_default() { runtime.load_ahead_images(images); }
}

/// The image command line, if it is one: `[options] <x>.qimg ...`.
pub fn route(arguments: &[String]) -> Option<Result<ExitCode, String>> {
    // Options (`--b-jit`, `--no-jit`) may precede the image, as they precede a script.
    let at = arguments.iter().position(|argument| !argument.starts_with("--"))?;
    let (options, [image, rest @ ..]) = arguments.split_at(at) else { return None };
    image.ends_with(EXTENSION).then(|| run(options, image, rest.iter().cloned()))
}
