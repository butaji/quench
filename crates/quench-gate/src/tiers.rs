//! The execution modes the gate runs programs in: the three Quench ships (user, 2026-11-22). The
//! interpreter runs alone (`--no-jit`); the default (no flag) puts each function in its
//! predicted-cheapest tier; AOT is the `quench aot` build, which compiles the program ahead of time
//! with u-jit over everything and runs as its own executable. There is no other mode.

use std::{ffi::OsString, path::Path};

use crate::{build::NODE, config::Config, process};

/// The flag that runs the interpreter alone.
pub const INTERPRETER: &str = "--no-jit";
/// The default mode's name (it has no flag).
pub const DEFAULT: &str = "default";
/// The AOT mode's name (it has no flag: the program is compiled, then its executable runs).
pub const AOT: &str = "aot";
/// The AOT compile flag of the AOT-enabled `quench`, which writes an executable to `-o`.
const COMPILE: &str = "--build-aot";
const OUTPUT: &str = "-o";

/// The modes trunk must pass whole suites in, with their runner flag (none for the default). AOT's
/// suites are the aot step's (`--suites`).
pub const TRUNK_MODES: [(&str, Option<&str>); 2] = [("no-jit", Some(INTERPRETER)), ("default", None)];

/// The argv that runs `script` in `mode` (`INTERPRETER`, `DEFAULT` or `AOT`), built once per
/// script: the host with the mode's flag and then `options` and the script, or for AOT the
/// executable the AOT compile makes of `script` in `scratch`, which takes no `options` (the
/// compiled program has no host flags). Callers append the script's own arguments.
pub fn launch(config: &Config, host: &Path, mode: &str, script: &Path, scratch: &Path, options: &[&str]) -> Result<Vec<OsString>, String> {
    if mode == AOT {
        let stem = script.file_stem().and_then(|stem| stem.to_str()).unwrap_or("program");
        let output = scratch.join(format!("{stem}.aot"));
        let compiler = config.aot_binary(NODE)?;
        let compiled = process::run(config.command(compiler).arg(COMPILE).arg(script).arg(OUTPUT).arg(&output))?;
        if !compiled.success {
            return Err(format!("{}: the AOT compile failed\n{}", script.display(), compiled.excerpt()));
        }
        return Ok(vec![output.into_os_string()]);
    }
    let mut argv: Vec<OsString> = vec![host.into()];
    if mode != DEFAULT {
        argv.push(mode.into());
    }
    argv.extend(options.iter().map(|option| OsString::from(*option)));
    argv.push(script.into());
    Ok(argv)
}
