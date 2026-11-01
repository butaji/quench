// The pinned Node source set: every file of Node's source tree the quench-node build reads,
// kept under `NODE_SOURCE_DIR` (relative to this crate) in Node's own layout. `build.rs`
// reads only these files; `sync-node-sources` (quench-node-test) copies exactly them from a
// Node checkout. Node's JavaScript library also includes `BUNDLED_DEPENDENCIES`
// (`node_source_dependencies.rs`).

/// The source set's root, relative to the quench-node crate.
#[allow(dead_code)]
pub(crate) const NODE_SOURCE_DIR: &str = "node";

/// The record of the checkout the source set was copied from: its Node version and commit.
#[allow(dead_code)]
pub(crate) const NODE_SOURCE_RECORD: &str = "SOURCE";

/// Node's license, which covers the library and its bundled dependencies.
#[allow(dead_code)]
pub(crate) const NODE_SOURCE_LICENSE: &str = "LICENSE";

/// The individual files the build reads: the version and option definitions, and the
/// protocol constant tables (compile cache, exit codes).
#[allow(dead_code)]
pub(crate) const NODE_SOURCE_FILES: &[&str] = &[
    "src/node_version.h",
    "src/node_options.h",
    "src/node_options.cc",
    "src/compile_cache.h",
    "src/node_exit_code.h",
];

/// The directories the build reads whole, with the file extensions it reads from them:
/// Node's JavaScript library, libuv's error definitions (probed for the target's values),
/// and the nghttp2 implementation the build compiles.
#[allow(dead_code)]
pub(crate) const NODE_SOURCE_TREES: &[(&str, &[&str])] = &[
    ("lib", &["js"]),
    ("deps/uv/include", &["h"]),
    ("deps/nghttp2/lib", &["c", "h"]),
];
