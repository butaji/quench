#!/usr/bin/env python3
"""Verification twin of crates/quench-gate/src/aot/program.rs.

Composes the JavaScript program for one C analogue exactly as the gate does, from the same
`// jetstream-*:` header lines, and optionally runs it under node.

usage: compose_row.py <analogue.c|.cc> <jetstream-root> <out.js>      emit the program
       compose_row.py <analogue.c|.cc> <jetstream-root> --run [node]  emit to a temp file, run it
"""
import os
import re
import subprocess
import sys
import tempfile

HEADER_LINES = 8
SEEDED_RANDOM = """(() => {
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
"""
REGEXP_ESCAPE = r'''if (!RegExp.escape) RegExp.escape = (text) => String(text).replace(/[\\^$.*+?()[\]{}|\/]/g, "\\$&");
'''
READ_BYTES = """const __jsRowBytes = (path) => {
  const fs = require("fs");
  if (path.endsWith(".z")) return require("zlib").inflateSync(fs.readFileSync(path));
  return fs.readFileSync(path);
};
"""
ASYNC_HELPERS = """JetStream.getBinary = async function (path) {
  const bytes = __jsRowBytes(path);
  return new Int8Array(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length));
};
JetStream.getString = async function (path) { return __jsRowBytes(path).toString("utf8"); };
"""
# The composed program is a classic global script, as JetStream's shells run it: `this` is the global
# object, top-level declarations are global, and no module wrapper is around it. Node and bun start
# a CommonJS file, so the host runner sets `require` as a global (the host surface the program's
# readers use) and evaluates the program's text in the global realm.
RUNNER = """globalThis.require = require;
require('vm').runInThisContext(require('fs').readFileSync(process.argv[2], 'utf8'), { filename: process.argv[2] });
"""
# The shell's Worker: each worker runs its script in its own realm (a vm context, as a browser worker
# has its own global), with postMessage delivered to the parent's onmessage on a later turn.
WORKER_SHIM = """(() => {
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
"""
WASM_PRERUN = """globalThis.quit = globalThis.abort = () => {};
globalThis.Module = { preRun: [], postRun: [], noInitialRun: true, print: () => {}, printErr: () => {} };
"""


def header_fields(text):
    """The `// jetstream-*` and `// iterations:` header lines, as {key: value}."""
    fields = {}
    for line in text.splitlines()[:HEADER_LINES]:
        if line.startswith("// "):
            body = line[3:]
            key, _, value = body.partition(": ")
            if key.startswith("jetstream-") or key == "iterations":
                fields[key] = value.strip()
    return fields


DRIVER_KEYS = ("class", "deterministic", "preload", "exports", "group", "namespace", "worker", "runner")


def driver_options(text):
    """The `key[=value]` fields of a `jetstream-driver` header; a bare key is `true`."""
    options = {}
    for token in text.split():
        key, _, value = token.partition("=")
        if key not in DRIVER_KEYS:
            raise ValueError(f"unknown driver field {key}")
        options[key] = value if "=" in token else "true"
    return options


def analogue(path, root):
    text = open(path, encoding="utf8").read()
    fields = header_fields(text)
    row = fields["jetstream-row"]
    files = fields["jetstream-files"].split()
    iterations = int(fields["iterations"])
    options = driver_options(fields.get("jetstream-driver", ""))
    if options.get("class", "Default") not in ("Default", "Async", "WasmEMCC"):
        raise ValueError(f"unknown benchmark class {options['class']}")
    preload = [entry.split("=", 1) for entry in options.get("preload", "").split(",") if entry]
    return dict(
        row=row,
        files=files,
        iterations=iterations,
        cls=options.get("class", "Default"),
        deterministic=options.get("deterministic", "false") == "true",
        preload=[(name, os.path.join(root, value)) for name, value in preload],
        exports=[name for name in options.get("exports", "").split(",") if name],
        group=options.get("group", ""),
        namespaces=[tuple(entry.split("@", 1)) for entry in options.get("namespace", "").split(",") if entry],
        worker=options.get("worker", "false") == "true",
        runner=options.get("runner", ""),
        root=root,
    )


def js_string(text):
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def prelude(a):
    preload = ", ".join(f"{name}: {js_string(path)}" for name, path in a["preload"])
    lines = [
        "globalThis.print = globalThis.print ?? console.log;",
        # The row's own files may silence console (web-ssr does); the composer's completion line uses this.
        "const __jsRowLog = console.log.bind(console);",
        "globalThis.self = globalThis;",
        # A failed console.assert is a failed validation: it must exit the program non-zero.
        "console.assert = (condition, ...details) => { if (!condition) throw new Error(\"Assertion failed: \" + details.join(\" \")); };",
        REGEXP_ESCAPE,
        READ_BYTES,
        f"globalThis.JetStream = {{ preload: {{ {preload} }}, resources: {{ {preload} }} }};",
    ]
    if a["cls"] in ("Async", "WasmEMCC"):
        lines.append(ASYNC_HELPERS)
    if a["cls"] == "WasmEMCC":
        lines.append(WASM_PRERUN)
    if a["deterministic"]:
        lines.append(SEEDED_RANDOM)
    if a["worker"]:
        # The row runs only in a browser (JetStream's exposeBrowserTest): its Workers are the shim below.
        lines.append(WORKER_SHIM + "globalThis.JetStream.isInBrowser = true;\n")
    return "\n".join(lines)


def file_text(a, file):
    path = os.path.join(a["root"], file)
    if not file.endswith(".z"):
        return open(path, encoding="utf8").read()
    exports = "".join(f"globalThis.{name} = {name};" for name in a["exports"])
    return f"(0, eval)(__jsRowBytes({js_string(path)}).toString(\"utf8\") + \"\\n;{exports}\");\n"


def namespace_text(a, file):
    """A UMD bundle that sees a CommonJS `module` sets its namespace there and not on the global; in
    the global script the bundle sets its own global, so this binds nothing unless `module` exists."""
    return "".join(f"if (typeof module !== \"undefined\") globalThis.{name} = module.exports;\n" for name, bundle in a["namespaces"] if bundle == file)


def expose_text(a, file):
    """A plain file's top-level declarations named by `exports=` become globals, as in a shell, where
    a file is a global script and its functions are visible to the eval'd code of later files."""
    if file.endswith(".z"):
        return ""
    return "".join(f"if (typeof {name} !== \"undefined\") globalThis.{name} = {name};\n" for name in a["exports"])


def reset(a):
    return "Math.random.__resetSeed();" if a["deterministic"] else ""


def run_body(a, count, report=True):
    """The driver's loop over `Benchmark` (sync or async), as JetStream's runner does."""
    row, n = a["row"], count
    done = f"__jsRowLog({js_string(row + ' ' + str(n) + ' iterations')});" if report else ""
    if a["runner"] == "wsl":
        return f"""{{
  for (let i = 0; i < {n}; i++) {{
    const benchmark = new Benchmark({{ iterationCount: {n} }});
    benchmark.buildStdlib();
    benchmark.run();
  }}
  {done}
}}
"""
    if a["cls"] == "Default":
        return f"""{{
  const benchmark = new Benchmark({{ iterationCount: {n} }});
  for (let i = 0; i < {n}; i++) {{
    benchmark.prepareForNextIteration?.();
    {reset(a)}
    benchmark.runIteration(i);
  }}
  benchmark.validate?.({n});
  {done}
}}
"""
    return f"""async function doRun() {{
  const benchmark = new Benchmark({{ iterationCount: {n} }});
  await benchmark.init?.();
  for (let i = 0; i < {n}; i++) {{
    await benchmark.prepareForNextIteration?.();
    {reset(a)}
    await benchmark.runIteration(i);
  }}
  benchmark.validate?.({n});
  {done}
}}
doRun().catch((error) => {{ throw error; }});
"""


def compose(a):
    parts = [f"// composed for {a['row']}\n", prelude(a)]
    if a["group"] == "file":
        for file in a["files"]:
            parts.append("{\n" + file_text(a, file) + run_body(dict(a, cls="Default"), a["iterations"], False) + "}\n")
        parts.append(f"__jsRowLog({js_string(a['row'] + ' ' + str(a['iterations']) + ' iterations')});\n")
        return "\n".join(parts)
    parts.extend(file_text(a, file) + "\n" + namespace_text(a, file) + expose_text(a, file) for file in a["files"])
    parts.append(run_body(a, a["iterations"]))
    return "\n".join(parts)


def main(argv):
    source, root = argv[1], argv[2]
    a = analogue(source, root)
    text = compose(a)
    if len(argv) > 3 and argv[3] == "--run":
        node = argv[4] if len(argv) > 4 else "/opt/node22/bin/node"
        with tempfile.TemporaryDirectory() as directory:
            script = os.path.join(directory, "program.js")
            runner = os.path.join(directory, "runner.js")
            open(script, "w", encoding="utf8").write(text)
            open(runner, "w", encoding="utf8").write(RUNNER)
            result = subprocess.run([node, runner, script], capture_output=True, text=True)
            sys.stdout.write(result.stdout[-400:])
            sys.stderr.write(result.stderr[-1500:])
            return result.returncode
    open(argv[3], "w", encoding="utf8").write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
