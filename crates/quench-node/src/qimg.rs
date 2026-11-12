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

thread_local! { static STAGED: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) }; }

/// `compile <entry.js> -o <out.qimg> [entry args]`.
pub fn compile(arguments: Vec<String>) -> Result<ExitCode, String> {
    let (entry, rest) = arguments.split_first().ok_or("usage: quench-node compile <entry.js> -o <out.qimg>")?;
    let flag = rest.iter().position(|argument| argument == "-o").ok_or("compile needs -o <out.qimg>")?;
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
    std::fs::write(out, file).map_err(|error| format!("{out}: {error}"))?;
    status
}

/// `<x>.qimg [args]`: stage the image file's programs, then run its entry.
pub fn run(image: &str, arguments: impl IntoIterator<Item = String>) -> Result<ExitCode, String> {
    let file = std::fs::read(image).map_err(|error| format!("{image}: {error}"))?;
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
    match arguments {
        [command, rest @ ..] if command == "compile" && !cfg!(feature = "aot") => Some(compile(rest.to_vec())),
        [image, rest @ ..] if image.ends_with(EXTENSION) => Some(run(image, rest.iter().cloned())),
        _ => None,
    }
}
