//! Node library lookups. Quench carries no Node code, so every lookup misses; the
//! callers that still request one fail until their module has a Rust implementation.
use std::borrow::Cow;
use std::path::Path;
pub(crate) enum LibrarySource {
    Configured,
    Bundled,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SourceView<'a> {
    Policy {
        exports: &'a [&'a str],
        supplied: &'a [&'a str],
    },
    Declaration {
        name: &'a str,
        asynchronous: bool,
    },
    FastBuffer,
    DomainTrampoline,
    CryptoHashFactory,
    CryptoRandomAliasFactory,
    CryptoSignatureFactory,
}

impl crate::NodeHost {
    pub(crate) fn node_library_view(
        &mut self,
        path: &Path,
        preference: LibrarySource,
        view: SourceView<'_>,
        project: impl FnOnce(&str) -> Result<String, String>,
    ) -> Result<Cow<'static, str>, String> {
        self.with_source_view(path, view, |host| {
            let source = host
                .node_library_source(path, preference)
                .map_err(|error| error.to_string())?;
            project(&source)
        })
    }

    pub(crate) fn with_source_view(
        &mut self,
        path: &Path,
        view: SourceView<'_>,
        project: impl FnOnce(&mut Self) -> Result<String, String>,
    ) -> Result<Cow<'static, str>, String> {
        #[cfg(feature = "native-execution")]
        let key = serde_json::to_string(&view).map_err(|error| error.to_string())?;
        #[cfg(not(feature = "native-execution"))]
        let _ = (path, view);
        #[cfg(feature = "native-execution")]
        if let crate::aot::ModuleAssetState::Embedded { modules, .. } = &self.module_assets {
            return modules.view(path, &key).map(Cow::Borrowed).ok_or_else(|| {
                format!(
                    "SDK source view outside frozen image: {} {key}",
                    path.display()
                )
            });
        }
        #[cfg(any(
            not(feature = "native-execution"),
            feature = "bundled-sources",
            feature = "aot"
        ))]
        {
            let projected = project(self)?;
            #[cfg(feature = "aot")]
            self.record_source_view(path, &key, &projected)?;
            Ok(Cow::Owned(projected))
        }
        #[cfg(all(
            feature = "native-execution",
            not(any(feature = "bundled-sources", feature = "aot"))
        ))]
        {
            let _ = project;
            Err(format!(
                "SDK source projection is unavailable: {} {key}",
                path.display()
            ))
        }
    }

    pub(crate) fn node_library_source(
        &mut self,
        path: &Path,
        _preference: LibrarySource,
    ) -> std::io::Result<Cow<'static, str>> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Node library source is not part of quench: {}", path.display()),
        ))
    }
}
