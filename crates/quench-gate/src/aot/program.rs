//! The JavaScript program an analogue's row runs as, composed the way JetStream's shell driver
//! composes it: the host surface and preloads, the seeded `Math.random` when the row seeds it, the
//! row's files in JetStream's load order, then the driver's loop over the row's `Benchmark` for the
//! analogue's iterations, which validates and names what ran. `tools/compose_row.py` is the twin.
//! The program is one classic script, as JetStream's shells run each file: `this` is the global
//! object and a file's top-level bindings are shared with the files after it.

use std::{fs, path::Path};

use super::{
    analogue::Analogue,
    driver::{Class, Driver},
};

/// JetStream's deterministic `Math.random` (the driver's `addDeterministicRandom`).
const SEEDED_RANDOM: &str = "(() => {
  const initialSeed = 49734321;
  let seed = initialSeed;
  Math.random = () => {
    seed = ((seed + 0x7ed55d16) + (seed << 12)) & 0xffff_ffff;
    seed = ((seed ^ 0xc761c23c) ^ (seed >>> 19)) & 0xffff_ffff;
    seed = ((seed + 0x165667b1) + (seed << 5)) & 0xffff_ffff;
    seed = ((seed + 0xd3a2646c) ^ (seed << 9)) & 0xffff_ffff;
    seed = ((seed + 0xfd7046c5) + (seed << 3)) & 0xffff_ffff;
    seed = ((seed ^ 0xb55a4f09) ^ (seed >>> 16)) & 0xffff_ffff;
    return (seed >>> 0) / 0x1_0000_0000;
  };
  Math.random.__resetSeed = () => { seed = initialSeed; };
})();
";

/// The shell's globals: `print`, `self`, and a failed `console.assert` as a thrown validation
/// failure. The program is a classic global script, as the shell runs it: no module wrapper, and
/// `require` is the host's global that the readers use.
const HOST_SURFACE: &str = "globalThis.print = globalThis.print ?? console.log;
const __jsRowLog = console.log.bind(console);
globalThis.self = globalThis;
console.assert = (condition, ...details) => { if (!condition) throw new Error(\"Assertion failed: \" + details.join(\" \")); };
";

/// `RegExp.escape` (ES2025, which node 22 lacks), for the startup rows' `StartupBenchmark`.
const REGEXP_ESCAPE: &str = r#"if (!RegExp.escape) RegExp.escape = (text) => String(text).replace(/[\\^$.*+?()[\]{}|\/]/g, "\\$&");
"#;

/// The shell's `Worker`: a worker runs its script in its own realm (a vm context, as a browser
/// worker has its own global), and each `postMessage` reaches the other side's `onmessage` on a
/// later turn. `JetStream.isInBrowser` is set because the row runs only in a browser.
const WORKER_SHIM: &str = r#"(() => {
  const vm = require("vm");
  const fs = require("fs");
  globalThis.Worker = class Worker {
    constructor(file) {
      this.onmessage = null;
      const parent = this;
      const sandbox = {
        console,
        setTimeout,
        clearTimeout,
        importScripts() {},
        close() {},
        postMessage(data) {
          setImmediate(() => { if (parent.onmessage) parent.onmessage({ data: structuredClone(data) }); });
        },
      };
      sandbox.self = sandbox;
      this.context = vm.createContext(sandbox);
      const source = fs.readFileSync(file, "utf8");
      setImmediate(() => vm.runInContext(source, this.context, { filename: file }));
    }
    postMessage(data) {
      const context = this.context;
      setImmediate(() => { if (context.onmessage) context.onmessage({ data: structuredClone(data) }); });
    }
    terminate() {}
  };
})();
globalThis.JetStream.isInBrowser = true;
"#;

/// The driver loop of `runner=wsl` (WSLBenchmark): a fresh `Benchmark` per iteration, its stdlib
/// built and its main run timed. `{count}`, `{done}`.
const WSL_LOOP: &str = "{
  for (let i = 0; i < {count}; i++) {
    const benchmark = new Benchmark({ iterationCount: {count} });
    benchmark.buildStdlib();
    benchmark.run();
  }
  {done}
}
";

/// Reads a resource as bytes; a `.z` resource is inflated, as the driver's prefetch does.
const READ_BYTES: &str = "const __jsRowBytes = (path) => {
  const fs = require(\"fs\");
  if (path.endsWith(\".z\")) return require(\"zlib\").inflateSync(fs.readFileSync(path));
  return fs.readFileSync(path);
};
";

/// The async classes' `JetStream.getBinary` and `getString` (the driver's `AsyncBenchmark`).
const ASYNC_HELPERS: &str = "JetStream.getBinary = async function (path) {
  const bytes = __jsRowBytes(path);
  return new Int8Array(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length));
};
JetStream.getString = async function (path) { return __jsRowBytes(path).toString(\"utf8\"); };
";

/// The Emscripten `Module` the `WasmEMCCBenchmark` prerun installs; its output is silenced.
const WASM_PRERUN: &str = "globalThis.quit = globalThis.abort = () => {};
globalThis.Module = { preRun: [], postRun: [], noInitialRun: true, print: () => {}, printErr: () => {} };
";

/// The synchronous driver loop of `DefaultBenchmark`: `{count}`, `{reset}`, `{done}`.
const DEFAULT_LOOP: &str = "{
  const benchmark = new Benchmark({ iterationCount: {count} });
  for (let i = 0; i < {count}; i++) {
    benchmark.prepareForNextIteration?.();
    {reset}
    benchmark.runIteration(i);
  }
  benchmark.validate?.({count});
  {done}
}
";

/// The awaited driver loop of the async classes: `{count}`, `{reset}`, `{done}`.
const ASYNC_LOOP: &str = "async function doRun() {
  const benchmark = new Benchmark({ iterationCount: {count} });
  await benchmark.init?.();
  for (let i = 0; i < {count}; i++) {
    await benchmark.prepareForNextIteration?.();
    {reset}
    await benchmark.runIteration(i);
  }
  benchmark.validate?.({count});
  {done}
}
doRun().catch((error) => { throw error; });
";

/// A JavaScript string literal for `text` (paths and row names are plain ASCII).
fn js_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The host surface, the preloads, the class's helpers and the seeding, before the files.
fn prelude(analogue: &Analogue, jetstream: &Path) -> String {
    let driver = &analogue.driver;
    let preload = driver
        .preload
        .iter()
        .map(|(name, path)| format!("{name}: {}", js_string(&jetstream.join(path).display().to_string())))
        .collect::<Vec<_>>()
        .join(", ");
    let mut text = String::from(HOST_SURFACE);
    text.push_str(REGEXP_ESCAPE);
    text.push_str(READ_BYTES);
    text.push_str(&format!("globalThis.JetStream = {{ preload: {{ {preload} }}, resources: {{ {preload} }} }};\n"));
    if driver.class != Class::Default {
        text.push_str(ASYNC_HELPERS);
    }
    if driver.class == Class::WasmEmcc {
        text.push_str(WASM_PRERUN);
    }
    if driver.deterministic {
        text.push_str(SEEDED_RANDOM);
    }
    if driver.worker {
        text.push_str(WORKER_SHIM);
    }
    text
}

/// One of the row's files as script text: a `.z` file is inflated and run as a script whose
/// top-level `let` bindings named in `exports` become globals.
fn file_text(driver: &Driver, jetstream: &Path, file: &str) -> Result<String, String> {
    let path = jetstream.join(file);
    if !file.ends_with(".z") {
        let location = path.display();
        return fs::read_to_string(&path).map_err(|error| format!("{location}: {error}"));
    }
    let exports: String = driver.exports.iter().map(|name| format!("globalThis.{name} = {name};")).collect();
    Ok(format!(
        "(0, eval)(__jsRowBytes({}).toString(\"utf8\") + \"\\n;{exports}\");\n",
        js_string(&path.display().to_string())
    ))
}

/// The globals bound from `module.exports` after `file`, if the row names it as a namespace.
fn namespace_text(driver: &Driver, file: &str) -> String {
    driver
        .namespaces
        .iter()
        .filter(|(_, bundle)| bundle == file)
        .map(|(global, _)| {
            format!("if (typeof module !== \"undefined\") globalThis.{global} = module.exports;\n")
        })
        .collect()
}

/// The globals a plain file's top-level declarations named in `exports` become, after the file: a
/// shell's file is a global script, so its functions are visible to the eval'd code of later files.
fn expose_text(driver: &Driver, file: &str) -> String {
    if file.ends_with(".z") {
        return String::new();
    }
    driver.exports.iter().map(|name| format!("if (typeof {name} !== \"undefined\") globalThis.{name} = {name};\n")).collect()
}

/// The driver loop for `count` iterations of the row's `Benchmark`, reporting the row's line when
/// `report` is set.
fn run_body(driver: &Driver, row: &str, count: u64, report: bool) -> String {
    let reset = if driver.deterministic { "Math.random.__resetSeed();" } else { "" };
    let done = if report { format!("__jsRowLog({});", js_string(&format!("{row} {count} iterations"))) } else { String::new() };
    if driver.runner == "wsl" {
        return WSL_LOOP.replace("{count}", &count.to_string()).replace("{done}", &done);
    }
    let template = if driver.class == Class::Default { DEFAULT_LOOP } else { ASYNC_LOOP };
    template.replace("{count}", &count.to_string()).replace("{reset}", reset).replace("{done}", &done)
}

/// The program's text: the host surface and preloads, every file of the row, then the driver. A
/// grouped row runs each file's subtest in a block of its own, then reports once.
pub fn compose(analogue: &Analogue, jetstream: &Path) -> Result<String, String> {
    let driver = &analogue.driver;
    let (row, count) = (analogue.row.as_str(), analogue.iterations);
    let mut program = format!("// composed for {row}\n{}", prelude(analogue, jetstream));
    if driver.per_file {
        let subtest = Driver { deterministic: driver.deterministic, ..Driver::default() };
        for file in &analogue.files {
            program.push_str(&format!("{{\n{}", file_text(driver, jetstream, file)?));
            program.push_str(&run_body(&subtest, row, count, false));
            program.push_str("}\n");
        }
        program.push_str(&format!("__jsRowLog({});\n", js_string(&format!("{row} {count} iterations"))));
        return Ok(program);
    }
    for file in &analogue.files {
        program.push_str(&file_text(driver, jetstream, file)?);
        program.push('\n');
        program.push_str(&namespace_text(driver, file));
        program.push_str(&expose_text(driver, file));
    }
    program.push_str(&run_body(driver, row, count, true));
    Ok(program)
}
