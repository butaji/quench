//! The errors Node's ESM loader reports for unresolvable or unloadable imports
//! (lib/internal/modules/esm/resolve.js, lib/internal/errors.js).
use quench_runtime::{ModuleLoadError, ModuleLoadErrorKind as Kind};
use oxc_resolver::{FileMetadata, FileSystem, ResolverGeneric};
use std::path::{Path, PathBuf};

/// URL schemes the default ESM loader accepts besides builtins.
const SUPPORTED_SCHEMES: [&str; 2] = ["file", "data"];
/// Extensions the default ESM loader has a format for (`extensionFormatMap`), plus
/// TypeScript, which it strips outside node_modules.
const LOADABLE_EXTENSIONS: [&str; 5] = ["js", "mjs", "cjs", "json", "wasm"];
const TYPESCRIPT_EXTENSIONS: [&str; 3] = ["ts", "mts", "cts"];
const NODE_MODULES: &str = "node_modules";

/// `ERR_UNKNOWN_BUILTIN_MODULE`.
pub(super) fn unknown_builtin(specifier: &str) -> ModuleLoadError {
    ModuleLoadError::new(Kind::Error, "ERR_UNKNOWN_BUILTIN_MODULE", format!("No such built-in module: {specifier}"))
}

/// `ERR_UNSUPPORTED_ESM_URL_SCHEME` for an absolute URL of another scheme.
pub(super) fn unsupported_scheme(specifier: &str) -> Option<ModuleLoadError> {
    let url = url::Url::parse(specifier).ok()?;
    // A one-letter scheme is a Windows drive path, not a URL.
    if url.scheme().len() == 1 || SUPPORTED_SCHEMES.contains(&url.scheme()) || url.scheme() == "node" {
        return None;
    }
    Some(ModuleLoadError::new(
        Kind::Error,
        "ERR_UNSUPPORTED_ESM_URL_SCHEME",
        format!(
            "Only URLs with a scheme in: file and data are supported by the default ESM loader. Received protocol '{}:'",
            url.scheme()
        ),
    ))
}

/// `ERR_MODULE_NOT_FOUND` for a file (`finalizeResolution`), which carries the file's `url`.
pub(super) fn module_not_found(path: &Path, referrer: &Path) -> ModuleLoadError {
    let error = ModuleLoadError::new(
        Kind::Error,
        "ERR_MODULE_NOT_FOUND",
        format!("Cannot find module '{}' imported from {}", path.display(), referrer.display()),
    );
    match url::Url::from_file_path(path) {
        Ok(url) => error.with_field("url", url),
        Err(()) => error,
    }
}

/// A bare specifier's package name: its first segment, or two for a scope.
fn package_name(specifier: &str) -> &str {
    let mut ends = specifier.match_indices('/').map(|(index, _)| index);
    let end = if specifier.starts_with('@') { ends.nth(1) } else { ends.next() };
    &specifier[..end.unwrap_or(specifier.len())]
}

/// The file Node names when a bare specifier's package exists but resolves to none.
pub(super) enum MissingPackageFile {
    /// The `exports` target (`finalizeResolution`), in its `package`.
    ExportsTarget { target: PathBuf, package: PathBuf },
    /// The package root without `exports`: its `main`, or `index.js` (`LegacyMainResolve`).
    LegacyMain(PathBuf),
    /// A subpath of a package without `exports`.
    Subpath(PathBuf),
}

/// The package directory a bare specifier names, searching node_modules up from `base`.
fn package_directory(base: &Path, name: &str) -> Option<PathBuf> {
    base.ancestors().map(|directory| directory.join(NODE_MODULES).join(name)).find(|directory| directory.is_dir())
}

/// Classify a bare specifier the resolver found no file for; `None` when no package
/// directory exists.
pub(super) fn missing_package_file<Fs: FileSystem>(
    resolver: &ResolverGeneric<Fs>, base: &Path, specifier: &str,
) -> Option<MissingPackageFile> {
    let name = package_name(specifier);
    let package = package_directory(base, name)?;
    let manifest = std::fs::read_to_string(package.join("package.json")).ok();
    let manifest = manifest.and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let subpath = specifier[name.len()..].trim_start_matches('/');
    if manifest.as_ref().is_some_and(|manifest| manifest.get("exports").is_some_and(|exports| !exports.is_null())) {
        let probe = ResolverGeneric::new_with_file_system(PresumedFiles { package: package.clone() }, resolver.options().clone());
        if let Ok(resolution) = probe.resolve(base, specifier) {
            return Some(MissingPackageFile::ExportsTarget { target: resolution.full_path(), package });
        }
    }
    if subpath.is_empty() {
        let main = manifest.as_ref().and_then(|manifest| manifest.get("main")).and_then(serde_json::Value::as_str);
        return Some(MissingPackageFile::LegacyMain(match main {
            Some(main) => normalize(&package.join(main)),
            None => package.join(LEGACY_INDEX),
        }));
    }
    Some(MissingPackageFile::Subpath(package.join(subpath)))
}

/// The file `LegacyMainResolve` names for a package with neither `main` nor index.
const LEGACY_INDEX: &str = "index.js";

/// `path.resolve`'s lexical `.` and `..` handling.
fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}

/// The real filesystem, except that a missing path inside `package` is presented as a
/// file. Resolving through it yields the `exports` target Node reports as missing.
struct PresumedFiles {
    package: PathBuf,
}

impl PresumedFiles {
    fn presume<T>(&self, path: &Path, real: std::io::Result<T>, presumed: impl FnOnce() -> T) -> std::io::Result<T> {
        match real {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && path.starts_with(&self.package) => Ok(presumed()),
            other => other,
        }
    }
}

impl FileSystem for PresumedFiles {
    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
    fn metadata(&self, path: &Path) -> std::io::Result<FileMetadata> {
        self.presume(path, std::fs::metadata(path).map(FileMetadata::from), || FileMetadata::new(true, false, false))
    }
    fn symlink_metadata(&self, path: &Path) -> std::io::Result<FileMetadata> {
        self.presume(path, std::fs::symlink_metadata(path).map(FileMetadata::from), || FileMetadata::new(true, false, false))
    }
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.presume(path, std::fs::canonicalize(path), || path.to_path_buf())
    }
}

/// The failure for a specifier the resolver could not resolve from `base`.
pub(super) fn unresolved<Fs: FileSystem>(
    resolver: &ResolverGeneric<Fs>, error: oxc_resolver::ResolveError, specifier: &str, base: &Path, referrer: &Path,
) -> ModuleLoadError {
    use oxc_resolver::ResolveError as R;
    let from = referrer.display();
    match error {
        R::NotFound(_) if specifier.starts_with('.') || Path::new(specifier).is_absolute() => {
            module_not_found(&base.join(specifier), referrer)
        }
        R::NotFound(_) => match missing_package_file(resolver, base, specifier) {
            Some(MissingPackageFile::ExportsTarget { target: file, .. } | MissingPackageFile::Subpath(file)) => {
                module_not_found(&file, referrer)
            }
            Some(MissingPackageFile::LegacyMain(main)) => ModuleLoadError::new(
                Kind::Error,
                "ERR_MODULE_NOT_FOUND",
                format!("Cannot find package '{}' imported from {from}", main.display()),
            ),
            None => ModuleLoadError::new(
                Kind::Error,
                "ERR_MODULE_NOT_FOUND",
                format!("Cannot find package '{}' imported from {from}", package_name(specifier)),
            ),
        },
        other => match require_unresolved(&other) {
            // The ESM loader also names the importer.
            Some(error) => ModuleLoadError { message: format!("{} imported from {from}", error.message), ..error },
            None => other.to_string().into(),
        },
    }
}

/// The failure for loading a resolved file whose extension the loader has no format for.
pub(super) fn unloadable(path: &Path) -> Option<ModuleLoadError> {
    let extension = path.extension().and_then(|extension| extension.to_str())?;
    if TYPESCRIPT_EXTENSIONS.contains(&extension) {
        let url = url::Url::from_file_path(path).map(String::from).unwrap_or_else(|_| path.display().to_string());
        return node_modules_typescript(path).then(|| type_stripping_unsupported(&url));
    }
    if LOADABLE_EXTENSIONS.contains(&extension) {
        return None;
    }
    Some(ModuleLoadError::new(
        Kind::TypeError,
        "ERR_UNKNOWN_FILE_EXTENSION",
        format!("Unknown file extension \".{extension}\" for {}", path.display()),
    ))
}

/// The module types Node's loader assigns (lib/internal/modules/esm/assert.js): JSON
/// modules are `"json"`, every other format is JavaScript.
const JSON_TYPE: &str = "json";
const JAVASCRIPT_TYPE: &str = "javascript";

/// `validateAttributes`: a JSON module needs `type: "json"`, and a type must match.
pub(super) fn validate_type(url: &str, requested: &str) -> Result<(), ModuleLoadError> {
    let path = url::Url::parse(url).map_or_else(|_| url.to_owned(), |url| url.path().to_owned());
    let actual = if Path::new(&path).extension().is_some_and(|extension| extension == JSON_TYPE) {
        JSON_TYPE
    } else {
        JAVASCRIPT_TYPE
    };
    if requested == actual || ![JSON_TYPE, JAVASCRIPT_TYPE].contains(&requested) {
        return Ok(());
    }
    Err(if requested == JAVASCRIPT_TYPE {
        ModuleLoadError::new(
            Kind::TypeError,
            "ERR_IMPORT_ATTRIBUTE_MISSING",
            format!("Module \"{url}\" needs an import attribute of \"type: {actual}\""),
        )
    } else {
        ModuleLoadError::new(
            Kind::TypeError,
            "ERR_IMPORT_ATTRIBUTE_TYPE_INCOMPATIBLE",
            format!("Module \"{url}\" is not of type \"{requested}\""),
        )
    })
}

/// A TypeScript file Node will not strip: one under node_modules.
pub(super) fn node_modules_typescript(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| TYPESCRIPT_EXTENSIONS.contains(&extension))
        && path.components().any(|component| component.as_os_str() == NODE_MODULES)
}

/// `ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING`; the ESM loader names a URL, `require` a path.
pub(super) fn type_stripping_unsupported(location: &str) -> ModuleLoadError {
    ModuleLoadError::new(
        Kind::Error,
        "ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING",
        format!("Stripping types is currently unsupported for files under node_modules, for \"{location}\""),
    )
}

/// `require`'s report of a package exports failure, which names no importer.
pub(super) fn require_unresolved(error: &oxc_resolver::ResolveError) -> Option<ModuleLoadError> {
    use oxc_resolver::ResolveError as R;
    Some(match error {
        R::PackagePathNotExported(subpath, package_json) if subpath == "." => ModuleLoadError::new(
            Kind::Error,
            "ERR_PACKAGE_PATH_NOT_EXPORTED",
            format!("No \"exports\" main defined in {}", package_json.display()),
        ),
        R::PackagePathNotExported(subpath, package_json) => ModuleLoadError::new(
            Kind::Error,
            "ERR_PACKAGE_PATH_NOT_EXPORTED",
            format!("Package subpath '{subpath}' is not defined by \"exports\" in {}", package_json.display()),
        ),
        R::PackageImportNotDefined(import, package_json) => ModuleLoadError::new(
            Kind::TypeError,
            "ERR_PACKAGE_IMPORT_NOT_DEFINED",
            format!("Package import specifier \"{import}\" is not defined in package {}", package_json.display()),
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn package_names_keep_scopes() {
        assert_eq!(super::package_name("pkg/sub/x"), "pkg");
        assert_eq!(super::package_name("@scope/pkg/sub"), "@scope/pkg");
        assert_eq!(super::package_name("pkg"), "pkg");
    }
}
