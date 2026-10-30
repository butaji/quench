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

impl crate::NodeHost {
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
