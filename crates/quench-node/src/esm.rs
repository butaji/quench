use std::{
    fs,
    path::{Path, PathBuf},
};

use rquickjs::{
    loader::{Loader, Resolver},
    Ctx, Error, Module, Result,
};

fn package_entry(package_root: &Path, subpath: &str) -> Option<PathBuf> {
    if !subpath.is_empty() {
        return Some(package_root.join(subpath));
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(package_root.join("package.json")).ok()?).ok()?;
    let entry = manifest
        .get("exports")
        .and_then(|exports| match exports {
            serde_json::Value::String(value) => Some(value.as_str()),
            serde_json::Value::Object(exports) => exports
                .get("import")
                .or_else(|| exports.get("default"))
                .and_then(serde_json::Value::as_str),
            _ => None,
        })
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

fn builtin_source(name: &str) -> Option<String> {
    let builtin = name.strip_prefix("node:").unwrap_or(name);
    // ESM's default export is the existing CommonJS-compatible namespace. The
    // named exports cover the common surface used by upstream smoke fixtures;
    // unsupported named interop remains an ordinary module error.
    let names: &[&str] = match builtin {
        "path" => &[
            "basename",
            "delimiter",
            "dirname",
            "extname",
            "format",
            "isAbsolute",
            "join",
            "normalize",
            "parse",
            "posix",
            "relative",
            "resolve",
            "sep",
            "toNamespacedPath",
            "win32",
        ],
        "assert" => &[
            "AssertionError",
            "deepEqual",
            "deepStrictEqual",
            "doesNotMatch",
            "doesNotReject",
            "doesNotThrow",
            "equal",
            "fail",
            "ifError",
            "match",
            "notDeepEqual",
            "notDeepStrictEqual",
            "notEqual",
            "notStrictEqual",
            "ok",
            "rejects",
            "strict",
            "strictEqual",
            "throws",
        ],
        "events" => &[
            "EventEmitter",
            "EventEmitterAsyncResource",
            "addAbortListener",
            "captureRejectionSymbol",
            "captureRejections",
            "defaultMaxListeners",
            "errorMonitor",
            "getEventListeners",
            "getMaxListeners",
            "once",
            "setMaxListeners",
            "usingAsyncResource",
        ],
        "fs" | "fs/promises" => &[
            "access",
            "appendFile",
            "chmod",
            "close",
            "constants",
            "copyFile",
            "cp",
            "cpSync",
            "exists",
            "mkdir",
            "open",
            "glob",
            "globSync",
            "readFile",
            "readFileSync",
            "mkdtemp",
            "lstat",
            "lstatSync",
            "mkdirSync",
            "readlinkSync",
            "readdirSync",
            "rmSync",
            "statSync",
            "symlinkSync",
            "unlinkSync",
            "utimesSync",
            "writeFileSync",
            "rename",
            "rm",
            "stat",
            "unlink",
            "writeFile",
            "promises",
        ],
        "test" => &[
            "after",
            "afterEach",
            "before",
            "beforeEach",
            "describe",
            "it",
            "run",
            "skip",
            "test",
            "todo",
        ],
        "module" => &[
            "createRequire",
            "builtinModules",
            "isBuiltin",
            "register",
            "runMain",
        ],
        "timers/promises" => &["setTimeout", "setImmediate", "setInterval", "scheduler"],
        "url" => &[
            "URL",
            "URLSearchParams",
            "domainToASCII",
            "domainToUnicode",
            "fileURLToPath",
            "format",
            "parse",
            "pathToFileURL",
            "resolve",
            "urlToHttpOptions",
        ],
        "net" => &[
            "BlockList",
            "Server",
            "Socket",
            "Stream",
            "connect",
            "createConnection",
            "createServer",
            "getDefaultAutoSelectFamily",
            "getDefaultAutoSelectFamilyAttemptTimeout",
            "isIP",
            "isIPv4",
            "isIPv6",
            "setDefaultAutoSelectFamily",
            "setDefaultAutoSelectFamilyAttemptTimeout",
        ],
        _ => &[],
    };
    let mut source = format!("const __m = globalThis.require({name:?});\nexport default __m;\n");
    for export in names {
        source.push_str(&format!("export const {export} = __m.{export};\n"));
    }
    Some(source)
}

impl Loader for NodeLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js>> {
        if name.starts_with("node:")
            || matches!(
                name,
                "module"
                    | "assert"
                    | "events"
                    | "fs"
                    | "path"
                    | "test"
                    | "timers/promises"
                    | "url"
                    | "net"
            )
        {
            return builtin_source(name)
                .map(|source| Module::declare(ctx.clone(), name, source))
                .unwrap_or_else(|| Err(Error::new_loading(name)));
        }
        let path = name.strip_prefix("file:").unwrap_or(name);
        if path.ends_with("/common/index.mjs") {
            let common_js = Path::new(path).with_file_name("index.js");
            let source = format!(
                "const __c = globalThis.require({:?});\nconst __m = globalThis.require(\"module\");\nexport default __c;\nexport const allowGlobals = __c.allowGlobals;\nexport const buildType = __c.buildType;\nexport const canCreateSymLink = __c.canCreateSymLink;\nexport const childShouldThrowAndAbort = __c.childShouldThrowAndAbort;\nexport const createRequire = __m.createRequire;\nexport const enoughTestMem = __c.enoughTestMem;\nexport const escapePOSIXShell = __c.escapePOSIXShell;\nexport const expectsError = __c.expectsError;\nexport const expectWarning = __c.expectWarning;\nexport const getArrayBufferViews = __c.getArrayBufferViews;\nexport const getBufferSources = __c.getBufferSources;\nexport const getPort = () => __c.PORT;\nexport const getTTYfd = __c.getTTYfd;\nexport const hasCrypto = __c.hasCrypto;\nexport const hasQuic = __c.hasQuic;\nexport const hasInspector = __c.hasInspector;\nexport const hasSQLite = __c.hasSQLite;\nexport const hasFFI = __c.hasFFI;\nexport const hasLocalStorage = __c.hasLocalStorage;\nexport const hasIntl = __c.hasIntl;\nexport const hasTemporal = __c.hasTemporal;\nexport const hasIPv6 = __c.hasIPv6;\nexport const isAIX = __c.isAIX;\nexport const isAlive = __c.isAlive;\nexport const isFreeBSD = __c.isFreeBSD;\nexport const isIBMi = __c.isIBMi;\nexport const isInsideDirWithUnusualChars = __c.isInsideDirWithUnusualChars;\nexport const isLinux = __c.isLinux;\nexport const isOpenBSD = __c.isOpenBSD;\nexport const isMacOS = __c.isMacOS;\nexport const isRiscv64 = __c.isRiscv64;\nexport const isSunOS = __c.isSunOS;\nexport const isWindows = __c.isWindows;\nexport const localIPv6Hosts = __c.localIPv6Hosts;\nexport const mustCall = __c.mustCall;\nexport const mustCallAtLeast = __c.mustCallAtLeast;\nexport const mustNotCall = __c.mustNotCall;\nexport const mustNotMutateObjectDeep = __c.mustNotMutateObjectDeep;\nexport const mustSucceed = __c.mustSucceed;\nexport const nodeProcessAborted = __c.nodeProcessAborted;\nexport const parseTestMetadata = __c.parseTestMetadata;\nexport const PIPE = __c.PIPE;\nexport const platformTimeout = __c.platformTimeout;\nexport const printSkipMessage = __c.printSkipMessage;\nexport const runWithInvalidFD = __c.runWithInvalidFD;\nexport const skip = __c.skip;\nexport const skipIf32Bits = __c.skipIf32Bits;\nexport const skipIfEslintMissing = __c.skipIfEslintMissing;\nexport const skipIfInspectorDisabled = __c.skipIfInspectorDisabled;\nexport const skipIfSQLiteMissing = __c.skipIfSQLiteMissing;\nexport const spawnPromisified = __c.spawnPromisified;\nexport const sleepSync = __c.sleepSync;\n",
                common_js.to_string_lossy()
            );
            let source = source.replace(
                "export const hasCrypto = __c.hasCrypto;\n",
                "export const hasCrypto = __c.hasCrypto;\nexport const hasDtls = __c.hasDtls;\n",
            );
            let module = Module::declare(ctx.clone(), name, source)?;
            module.meta()?.set("url", format!("file://{}", path))?;
            return Ok(module);
        }
        let path = PathBuf::from(path);
        let extension = path.extension().and_then(|ext| ext.to_str());
        if extension == Some("js") && is_package_module(&path) {
            let module = Module::declare(ctx.clone(), name, fs::read(&path)?)?;
            module
                .meta()?
                .set("url", format!("file://{}", path.display()))?;
            return Ok(module);
        }
        if extension == Some("js") {
            let absolute = fs::canonicalize(&path).unwrap_or(path);
            let source = format!(
                "export default globalThis.require({:?});\n",
                absolute.to_string_lossy()
            );
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
        let module = Module::declare(ctx.clone(), name, fs::read(&path)?)?;
        module
            .meta()?
            .set("url", format!("file://{}", path.display()))?;
        Ok(module)
    }
}
