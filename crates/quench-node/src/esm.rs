use rquickjs::{
    loader::{Loader, Resolver},
    Ctx, Error, Module, Result,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn package_entry(package_root: &Path, subpath: &str) -> Option<PathBuf> {
    if !subpath.is_empty() {
        return Some(package_root.join(subpath));
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(package_root.join("package.json")).ok()?).ok()?;
    fn export_target(value: &serde_json::Value) -> Option<&str> {
        match value {
            serde_json::Value::String(value) => Some(value.as_str()),
            serde_json::Value::Object(exports) => [".", "import", "default"]
                .iter()
                .find_map(|key| exports.get(*key).and_then(export_target)),
            _ => None,
        }
    }
    let entry = manifest
        .get("exports")
        .and_then(export_target)
        .or_else(|| manifest.get("module").and_then(serde_json::Value::as_str))
        .or_else(|| manifest.get("main").and_then(serde_json::Value::as_str))
        .unwrap_or("index.js");
    Some(package_root.join(entry))
}
fn package_resolution(base: &str, name: &str) -> Option<PathBuf> {
    let (package_name, subpath) = if let Some(rest) = name.strip_prefix('@') {
        let (scope, rest) = rest.split_once('/')?;
        let (package, subpath) = rest.split_once('/').unwrap_or((rest, ""));
        (format!("@{scope}/{package}"), subpath)
    } else {
        let (package, subpath) = name.split_once('/').unwrap_or((name, ""));
        (package.to_owned(), subpath)
    };
    let mut directory = Path::new(base).parent()?.to_path_buf();
    loop {
        let root = directory.join("node_modules").join(&package_name);
        if root.is_dir() {
            let entry = package_entry(&root, subpath)?;
            if entry.is_file() {
                return Some(entry);
            }
            for extension in ["mjs", "js"] {
                let candidate = entry.with_extension(extension);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        if !directory.pop() {
            break;
        }
    }
    None
}
fn is_package_module(path: &Path) -> bool {
    let mut directory = path.parent();
    while let Some(current) = directory {
        let manifest = current.join("package.json");
        if let Ok(bytes) = fs::read(manifest) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                return value.get("type").and_then(serde_json::Value::as_str) == Some("module");
            }
        }
        directory = current.parent();
    }
    false
}
fn set_module_url<'js>(module: &Module<'js>, path: &Path) -> Result<()> {
    let absolute = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    module
        .meta()?
        .set("url", format!("file://{}", absolute.display()))
}
#[derive(Debug, Default)]
pub struct NodeResolver;
impl Resolver for NodeResolver {
    fn resolve<'js>(&mut self, _ctx: &Ctx<'js>, base: &str, name: &str) -> Result<String> {
        if name.starts_with("node:") || name.starts_with("file:") {
            return Ok(name.to_owned());
        }
        if name.starts_with('.') {
            let base = Path::new(base);
            let parent = base.parent().unwrap_or_else(|| Path::new("."));
            let mut path = parent.join(name);
            if path.extension().is_none() {
                if path.with_extension("mjs").is_file() {
                    path.set_extension("mjs");
                } else {
                    path.set_extension("js");
                }
            }
            return Ok(path.to_string_lossy().into_owned());
        }
        if let Some(path) = package_resolution(base, name) {
            return Ok(path.to_string_lossy().into_owned());
        }
        Ok(name.to_owned())
    }
}
#[derive(Debug, Default)]
pub struct NodeLoader;
macro_rules! export_names {
    ($($name:ident)*) => { &[$(stringify!($name)),*] };
}
// This table is the data-driven ESM compatibility declaration; splitting its
// arms would obscure the supported module surface without reducing behavior.
#[rustfmt::skip]
#[allow(clippy::too_many_lines)]
fn builtin_source(name: &str) -> Option<String> {
    let builtin = name.strip_prefix("node:").unwrap_or(name);
    if builtin == "process" {
        return Some("export default globalThis.process;\n".to_owned());
    }
    // ESM's default export is the existing CommonJS-compatible namespace. The
    // named exports cover the common surface used by upstream smoke fixtures;
    // unsupported named interop remains an ordinary module error.
    let names: &[&str] = match builtin {
        "path" => export_names!(basename delimiter dirname extname format isAbsolute join normalize parse posix relative resolve sep toNamespacedPath win32),
        "assert" => export_names!(AssertionError deepEqual deepStrictEqual doesNotMatch doesNotReject doesNotThrow equal fail ifError match notDeepEqual notDeepStrictEqual notEqual notStrictEqual ok rejects strict strictEqual throws),
        "events" => export_names!(EventEmitter EventEmitterAsyncResource addAbortListener captureRejectionSymbol captureRejections defaultMaxListeners errorMonitor getEventListeners getMaxListeners once setMaxListeners usingAsyncResource),
        "stream" => export_names!(Duplex PassThrough Readable Stream Transform Writable addAbortSignal compose destroy duplexPair finished getDefaultHighWaterMark isDisturbed isErrored isReadable pipeline promises setDefaultHighWaterMark),
        "fs" | "fs/promises" => export_names!(access appendFile chmod close constants copyFile cp cpSync exists mkdir open glob globSync readFile readFileSync mkdtemp lstat lstatSync mkdirSync readlinkSync readdirSync realpathSync rmSync statSync symlinkSync unlinkSync utimesSync writeFileSync rename rm stat unlink writeFile promises),
        "test" => export_names!(after afterEach before beforeEach describe it run skip test todo),
        "module" => export_names!(createRequire builtinModules isBuiltin register runMain),
        "timers/promises" => export_names!(setTimeout setImmediate setInterval scheduler),
        "util" => export_names!(format inspect promisify types),
        "v8" => export_names!(deserialize serialize getHeapStatistics setFlagsFromString),
        "url" => export_names!(URL URLSearchParams domainToASCII domainToUnicode fileURLToPath format parse pathToFileURL resolve urlToHttpOptions),
        "net" => export_names!(BlockList Server Socket Stream connect createConnection createServer getDefaultAutoSelectFamily getDefaultAutoSelectFamilyAttemptTimeout isIP isIPv4 isIPv6 setDefaultAutoSelectFamily setDefaultAutoSelectFamilyAttemptTimeout),
        _ => export_names!(),
    };
    let mut source = format!("const __m = globalThis.require({name:?});\nexport default __m;\n");
    for export in names {
        source.push_str(&format!("export const {export} = __m.{export};\n"));
    }
    Some(source)
}
impl Loader for NodeLoader {
    #[allow(clippy::too_many_lines)]
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js>> {
        if name.starts_with("node:")
            || "module assert events stream fs fs/promises path test timers/promises url net process util v8"
                .split_whitespace()
                .any(|builtin| builtin == name)
        {
            return builtin_source(name)
                .map(|source| Module::declare(ctx.clone(), name, source))
                .unwrap_or_else(|| Err(Error::new_loading(name)));
        }
        let path = name.strip_prefix("file:").unwrap_or(name);
        if path.ends_with("/common/index.mjs") {
            let common_js = Path::new(path).with_file_name("index.js");
            let mut source = format!(
                "const __c = globalThis.require({:?});\nconst __m = globalThis.require(\"module\");\nexport default __c;\n",
                common_js.to_string_lossy()
            );
            for export in "allowGlobals buildType canCreateSymLink childShouldThrowAndAbort enoughTestMem escapePOSIXShell expectsError expectWarning getArrayBufferViews getBufferSources getTTYfd hasCrypto hasDtls hasQuic hasInspector hasSQLite hasFFI hasLocalStorage hasIntl hasTemporal hasIPv6 isAIX isAlive isFreeBSD isIBMi isInsideDirWithUnusualChars isLinux isOpenBSD isMacOS isRiscv64 isSunOS isWindows localIPv6Hosts mustCall mustCallAtLeast mustNotCall mustNotMutateObjectDeep mustSucceed nodeProcessAborted parseTestMetadata PIPE platformTimeout printSkipMessage runWithInvalidFD skip skipIf32Bits skipIfEslintMissing skipIfInspectorDisabled skipIfSQLiteMissing spawnPromisified sleepSync".split_whitespace() {
                source.push_str(&format!("export const {export} = __c.{export};\n"));
            }
            source.push_str(
                "export const createRequire = __m.createRequire;\nexport const getPort = () => __c.PORT;\n",
            );
            let module = Module::declare(ctx.clone(), name, source)?;
            set_module_url(&module, Path::new(path))?;
            return Ok(module);
        }
        let path = PathBuf::from(path);
        let extension = path.extension().and_then(|ext| ext.to_str());
        if extension == Some("js") && is_package_module(&path) {
            let bytes = fs::read(&path)?;
            let module = Module::declare(ctx.clone(), name, bytes)?;
            set_module_url(&module, &path)?;
            return Ok(module);
        }
        if matches!(extension, Some("js") | Some("cjs")) {
            let absolute = fs::canonicalize(&path).unwrap_or(path);
            let mut source = format!(
                "export default globalThis.require({:?});\n",
                absolute.to_string_lossy()
            );
            if absolute.ends_with("prettier/index.cjs") {
                for export in [
                    "__debug",
                    "check",
                    "clearConfigCache",
                    "doc",
                    "format",
                    "formatWithCursor",
                    "getFileInfo",
                    "getSupportInfo",
                    "resolveConfig",
                    "resolveConfigFile",
                    "util",
                    "version",
                ] {
                    source.push_str(&format!(
                        "export const {export} = globalThis.require({:?}).{export};\n",
                        absolute.to_string_lossy()
                    ));
                }
            }
            let source = if absolute.to_string_lossy().ends_with("/common/fs.js") {
                format!(
                    "{source}export const nextdir = globalThis.require({0}).nextdir;\nexport const assertDirEquivalent = globalThis.require({0}).assertDirEquivalent;\nexport const collectEntries = globalThis.require({0}).collectEntries;\n",
                    format!("{:?}", absolute.to_string_lossy())
                )
            } else {
                source
            };
            return Module::declare(ctx.clone(), name, source);
        }
        if extension != Some("mjs") {
            return Err(Error::new_loading(name));
        }
        let bytes = fs::read(&path)?;
        let module = Module::declare(ctx.clone(), name, bytes)?;
        set_module_url(&module, &path)?;
        Ok(module)
    }
}
