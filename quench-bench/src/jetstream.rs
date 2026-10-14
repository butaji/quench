//! JetStream 3.0: the stock `cli.js` driver, one test per process.
//!
//! JetStream targets the d8, jsc and SpiderMonkey shells. The prelude gives
//! Node-compatible engines the same shell surface on `node:vm`: `print`,
//! `printErr`, `read`/`readFile`, `load`, and `runString`, which evaluates in
//! a fresh realm as JetStream requires for each benchmark. The driver,
//! iteration counts, validation and scoring stay JetStream's own.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// The pinned `WebKit/JetStream` checkout (branch `JetStream3.0`).
pub(crate) const SUITE_DIR: &str = "quench-bench/jetstream";

/// Tests in the lane's work order: server-side JavaScript first, then
/// server-side Wasm (Linear TOD-121).
pub(crate) const FIXTURES: &[&str] = &[
    "web-ssr",
    "typescript-lib",
    "babel-wtb",
    "babylon-wtb",
    "acorn-wtb",
    "espree-wtb",
    "esprima-next-wtb",
    "prettier-wtb",
    "postcss-wtb",
    "source-map-wtb",
    "babel-minify-wtb",
    "chai-wtb",
    "doxbee-promise",
    "doxbee-async",
    "async-fs",
    "sync-fs",
    "js-tokens",
    "lazy-collections",
    "json-parse-inspector",
    "json-stringify-inspector",
    "validatorjs",
    "UniPoker",
    "hash-map",
    "stanford-crypto-aes",
    "stanford-crypto-pbkdf2",
    "stanford-crypto-sha256",
    "bigint-noble-ed25519",
    "sqlite3-wasm",
    "zlib-wasm",
    "argon2-wasm",
];

/// Resources load on demand inside each benchmark realm, so no engine needs
/// WebAssembly to decompress prefetched resources (Node `--jitless` has none).
const DRIVER_ARGUMENTS: &[&str] = &["--no-prefetch"];

const PRELUDE: &str = r#""use strict";
// JetStream shell surface on node:vm. argv: <prelude> <jetstream-root> <cli args...>
const fs = require("fs");
const path = require("path");
const vm = require("vm");
const host = process;
const root = host.argv[2];
const resolve = (file) => path.resolve(root, file);
const readFile = (file, mode) =>
  mode === "binary" ? new Uint8Array(fs.readFileSync(resolve(file))) : fs.readFileSync(resolve(file), "utf8");
const print = (...args) => host.stdout.write(args.join(" ") + "\n");
const printErr = (...args) => host.stderr.write(args.join(" ") + "\n");
function shellSurface(global, evaluate) {
  global.print = print;
  global.printErr = printErr;
  global.read = readFile;
  global.readFile = readFile;
  global.load = (file) => evaluate(readFile(file), resolve(file));
  global.loadString = (source) => evaluate(source, "loadString");
  global.runString = (source) => {
    const context = vm.createContext({});
    const realm = vm.runInContext("globalThis", context);
    shellSurface(realm, (code, filename) => vm.runInContext(code, context, { filename }));
    for (const name of ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "queueMicrotask"])
      realm[name] = globalThis[name];
    realm.loadString(source);
    return realm;
  };
}
shellSurface(globalThis, (code, filename) => vm.runInThisContext(code, { filename }));
globalThis.arguments = host.argv.slice(3);
globalThis.load("cli.js");
"#;

pub(crate) fn fixtures() -> Vec<PathBuf> {
    FIXTURES.iter().map(|name| fixture_path(name)).collect()
}

/// A test is identified by its name under the corpus directory.
pub(crate) fn fixture_path(name: &str) -> PathBuf {
    Path::new(SUITE_DIR).join(name)
}

pub(crate) fn test_name(file: &Path) -> String {
    file.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn prelude() -> &'static [u8] {
    PRELUDE.as_bytes()
}

/// Engine arguments after the engine's own: the prelude, the corpus root and
/// the driver flags selecting one test.
pub(crate) fn arguments(file: &Path, prelude: &Path) -> Vec<OsString> {
    [prelude.as_os_str().to_owned(), SUITE_DIR.into()]
        .into_iter()
        .chain(DRIVER_ARGUMENTS.iter().map(OsString::from))
        .chain(std::iter::once(OsString::from(format!(
            "--test={}",
            test_name(file)
        ))))
        .collect()
}

/// The test's JetStream Score: `<name> Score <value> pts`.
pub(crate) fn score(file: &Path, stdout: &str) -> Option<f64> {
    let name = test_name(file);
    stdout.lines().find_map(|line| {
        let mut fields = line.strip_prefix(name.as_str())?.split_whitespace();
        (fields.next()? == "Score").then_some(())?;
        fields.next()?.parse().ok()
    })
}

/// Output that must agree across engines: everything except measurements.
pub(crate) fn semantic_output(stdout: &str) -> String {
    stdout
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.ends_with(" pts") && !line.ends_with(" ms"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_test_score_not_its_sub_scores() {
        let stdout = "Running web-ssr:\n\
            web-ssr First-Score                             56.04 pts\n\
            web-ssr Score                                   70.47 pts\n\
            web-ssr Wall-Time                             2453.34 ms \n\
            Overall Score                                   70.47 pts\n";
        let file = fixture_path("web-ssr");
        assert_eq!(score(&file, stdout), Some(70.47));
        assert_eq!(score(&fixture_path("zlib-wasm"), stdout), None);
        assert_eq!(semantic_output(stdout), "Running web-ssr:");
    }
}
