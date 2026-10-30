//! Immutable, pinned Node library input for the shared JavaScript interpreter.
use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
include!(concat!(env!("OUT_DIR"), "/node_sources.rs"));
include!("node_source_dependencies.rs");

pub(crate) fn resolve(path: &Path, root: &Path) -> Option<(PathBuf, &'static str)> {
    let relative = path.strip_prefix(root).ok()?;
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => normalized.push(name),
            Component::CurDir => (),
            Component::ParentDir if normalized.pop() => (),
            _ => return None,
        }
    }
    let relative = normalized.to_str()?.replace('\\', "/");
    let index = SOURCES
        .binary_search_by_key(&relative.as_str(), |(name, _)| *name)
        .ok()?;
    Some((root.join(SOURCES[index].0), SOURCES[index].1))
}

pub(crate) fn source(path: &Path, root: &Path) -> Option<&'static str> {
    resolve(path, root).map(|(_, source)| source)
}

pub(crate) enum LibrarySource {
    Configured,
    Bundled,
}

#[derive(serde::Serialize)]
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
        if let crate::aot::ModuleAssetState::Embedded(modules) = &self.module_assets {
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
        preference: LibrarySource,
    ) -> std::io::Result<Cow<'static, str>> {
        #[cfg(feature = "native-execution")]
        if let Some(file) = self.embedded_module(path) {
            return match std::str::from_utf8(file.bytes) {
                Ok(source) => Ok(Cow::Borrowed(source)),
                Err(_) => {
                    let mut invalid = file.bytes;
                    Err(
                        std::io::Read::read_to_string(&mut invalid, &mut String::new())
                            .expect_err("invalid UTF-8 cannot decode as library text"),
                    )
                }
            };
        }
        let source = match preference {
            LibrarySource::Configured => {
                std::fs::read_to_string(path)
                    .map(Cow::Owned)
                    .or_else(|error| {
                        source(path, &self.node_library_root)
                            .map(Cow::Borrowed)
                            .ok_or(error)
                    })
            }
            LibrarySource::Bundled => source(path, &self.node_library_root)
                .map(Cow::Borrowed)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Node library source is not bundled",
                    )
                }),
        }?;
        #[cfg(feature = "aot")]
        self.record_module_bytes(path, quench_runtime::SourceKind::Script, source.as_bytes())?;
        Ok(source)
    }
}

#[cfg(all(test, feature = "bundled-sources"))]
mod source_order_tests {
    use super::*;

    #[test]
    fn configured_and_bundled_consumers_keep_their_source_order() {
        let root =
            std::env::temp_dir().join(format!("quench-sdk-source-order-{}", std::process::id()));
        let path = root.join("lib/internal/buffer.js");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "configured source").unwrap();
        let mut host = crate::NodeHost::new(vec!["quench".into()]);
        host.node_library_root = root.clone();
        assert_eq!(
            host.node_library_source(&path, LibrarySource::Configured)
                .unwrap(),
            "configured source"
        );
        assert_eq!(
            host.node_library_source(&path, LibrarySource::Bundled)
                .unwrap(),
            source(&path, &root).unwrap()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, feature = "native-execution"))]
mod frozen_source_tests {
    use super::*;
    use crate::aot::{EmbeddedModuleFile, EmbeddedModules};

    #[test]
    fn frozen_views_skip_source_reads_and_projection_callbacks() {
        let exports = ["selected"];
        let key = serde_json::to_string(&SourceView::Policy {
            exports: &exports,
            supplied: &[],
        })
        .unwrap();
        let views = Box::leak(
            vec![crate::aot::EmbeddedSourceView {
                path: "/quench-frozen-sdk/lib/policy.js",
                key: Box::leak(key.into_boxed_str()),
                source: "function selected() {}",
            }]
            .into_boxed_slice(),
        );
        let modules = Box::leak(Box::new(EmbeddedModules {
            node_root: Some("/quench-frozen-sdk"),
            entry: None,
            files: &[],
            resolutions: &[],
            views,
        }));
        let mut host = crate::NodeHost::new(vec!["quench".into()]).with_embedded_modules(modules);
        let path = Path::new("/quench-frozen-sdk/lib/policy.js");
        let view = host
            .node_library_view(
                path,
                LibrarySource::Configured,
                SourceView::Policy {
                    exports: &exports,
                    supplied: &[],
                },
                |_| panic!("frozen projection callback executed"),
            )
            .unwrap();
        assert!(matches!(view, Cow::Borrowed("function selected() {}")));
        let other = ["other"];
        assert!(host
            .node_library_view(
                path,
                LibrarySource::Configured,
                SourceView::Policy {
                    exports: &other,
                    supplied: &[]
                },
                |_| panic!("missing view fell back to parsing")
            )
            .is_err());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn conflicting_view_capture_preserves_the_original_projection() {
        use std::{cell::RefCell, rc::Rc};
        let assets = Rc::new(RefCell::new(crate::aot::ModuleAssets::default()));
        let mut host = crate::NodeHost::new(vec!["quench".into()]);
        host.module_assets = crate::aot::ModuleAssetState::Capture(assets.clone());
        let path = Path::new("/quench-sdk/policy.js");
        let first = host
            .with_source_view(path, SourceView::FastBuffer, |_| {
                Ok("first projection".into())
            })
            .unwrap();
        assert_eq!(first, "first projection");
        assert!(host
            .with_source_view(path, SourceView::FastBuffer, |_| Ok(
                "different projection".into()
            ))
            .is_err());
        assert_eq!(assets.borrow().views.len(), 1);
        assert_eq!(
            assets.borrow().views.values().next().unwrap().as_ref(),
            "first projection"
        );
    }

    #[test]
    fn sdk_consumers_read_the_frozen_file_without_a_filesystem_source() {
        static MODULES: EmbeddedModules = EmbeddedModules {
            node_root: Some("/quench-frozen-sdk"),
            entry: None,
            files: &[EmbeddedModuleFile {
                path: "/quench-frozen-sdk/lib/example.js",
                bytes: b"frozen source",
                kind: quench_runtime::SourceKind::Script,
                prepared: None,
            }],
            resolutions: &[],
            views: &[],
        };
        let path = Path::new("/quench-frozen-sdk/lib/example.js");
        let mut host = crate::NodeHost::new(vec!["quench".into()]).with_embedded_modules(&MODULES);
        for preference in [LibrarySource::Configured, LibrarySource::Bundled] {
            assert!(matches!(
                host.node_library_source(path, preference).unwrap(),
                Cow::Borrowed("frozen source")
            ));
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn sdk_reads_are_captured_and_changing_inputs_are_rejected() {
        use std::{cell::RefCell, rc::Rc};
        let root =
            std::env::temp_dir().join(format!("quench-sdk-source-capture-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("policy.js");
        std::fs::write(&path, "first source").unwrap();
        let assets = Rc::new(RefCell::new(crate::aot::ModuleAssets::default()));
        let mut host = crate::NodeHost::new(vec!["quench".into()]);
        host.module_assets = crate::aot::ModuleAssetState::Capture(assets.clone());
        host.node_library_source(&path, LibrarySource::Configured)
            .unwrap();
        assert_eq!(assets.borrow().files[&path].bytes.as_ref(), b"first source");
        std::fs::write(&path, "changed source").unwrap();
        assert_eq!(
            host.node_library_source(&path, LibrarySource::Configured)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(assets.borrow().files[&path].bytes.as_ref(), b"first source");
        std::fs::remove_dir_all(root).unwrap();
    }
}
