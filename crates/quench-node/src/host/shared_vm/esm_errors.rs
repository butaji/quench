//! The errors Node's ESM loader reports for unresolvable or unloadable imports
//! (lib/internal/modules/esm/resolve.js, lib/internal/errors.js).
use quench_runtime::{ModuleLoadError, ModuleLoadErrorKind as Kind};
use std::path::Path;

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

/// `ERR_MODULE_NOT_FOUND` for a file.
pub(super) fn module_not_found(path: &Path, referrer: &Path) -> ModuleLoadError {
    ModuleLoadError::new(
        Kind::Error,
        "ERR_MODULE_NOT_FOUND",
        format!("Cannot find module '{}' imported from {}", path.display(), referrer.display()),
    )
}

/// A bare specifier's package name: its first segment, or two for a scope.
fn package_name(specifier: &str) -> &str {
    let mut ends = specifier.match_indices('/').map(|(index, _)| index);
    let end = if specifier.starts_with('@') { ends.nth(1) } else { ends.next() };
    &specifier[..end.unwrap_or(specifier.len())]
}

/// The failure for a specifier the resolver could not resolve from `base`.
pub(super) fn unresolved(error: oxc_resolver::ResolveError, specifier: &str, base: &Path, referrer: &Path) -> ModuleLoadError {
    use oxc_resolver::ResolveError as R;
    let from = referrer.display();
    match error {
        R::NotFound(_) if specifier.starts_with('.') || Path::new(specifier).is_absolute() => {
            module_not_found(&base.join(specifier), referrer)
        }
        R::NotFound(_) => {
            let name = package_name(specifier);
            // A found package with a missing file reports the file (`legacyMainResolve`).
            let package = base.ancestors().map(|directory| directory.join(NODE_MODULES).join(name)).find(|directory| directory.is_dir());
            match package {
                Some(directory) => module_not_found(&directory.join(&specifier[name.len()..].trim_start_matches('/')), referrer),
                None => ModuleLoadError::new(
                    Kind::Error,
                    "ERR_MODULE_NOT_FOUND",
                    format!("Cannot find package '{name}' imported from {from}"),
                ),
            }
        }
        R::PackagePathNotExported(subpath, package_json) if subpath == "." => ModuleLoadError::new(
            Kind::Error,
            "ERR_PACKAGE_PATH_NOT_EXPORTED",
            format!("No \"exports\" main defined in {} imported from {from}", package_json.display()),
        ),
        R::PackagePathNotExported(subpath, package_json) => ModuleLoadError::new(
            Kind::Error,
            "ERR_PACKAGE_PATH_NOT_EXPORTED",
            format!("Package subpath '{subpath}' is not defined by \"exports\" in {} imported from {from}", package_json.display()),
        ),
        R::PackageImportNotDefined(import, package_json) => ModuleLoadError::new(
            Kind::TypeError,
            "ERR_PACKAGE_IMPORT_NOT_DEFINED",
            format!("Package import specifier \"{import}\" is not defined in package {} imported from {from}", package_json.display()),
        ),
        other => other.to_string().into(),
    }
}

/// The failure for loading a resolved file whose extension the loader has no format for.
pub(super) fn unloadable(path: &Path) -> Option<ModuleLoadError> {
    let extension = path.extension().and_then(|extension| extension.to_str())?;
    if TYPESCRIPT_EXTENSIONS.contains(&extension) {
        if !path.components().any(|component| component.as_os_str() == NODE_MODULES) {
            return None;
        }
        let url = url::Url::from_file_path(path).map(String::from).unwrap_or_else(|_| path.display().to_string());
        return Some(ModuleLoadError::new(
            Kind::Error,
            "ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING",
            format!("Stripping types is currently unsupported for files under node_modules, for \"{url}\""),
        ));
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

#[cfg(test)]
mod tests {
    #[test]
    fn package_names_keep_scopes() {
        assert_eq!(super::package_name("pkg/sub/x"), "pkg");
        assert_eq!(super::package_name("@scope/pkg/sub"), "@scope/pkg");
        assert_eq!(super::package_name("pkg"), "pkg");
    }
}
