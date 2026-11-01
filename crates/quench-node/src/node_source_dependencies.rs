/// Node's bundled JavaScript modules and their physical source-tree paths.
pub(crate) const BUNDLED_DEPENDENCIES: &[(&str, &str)] = &[
    ("internal/deps/acorn/acorn/dist/acorn", "deps/acorn/acorn/dist/acorn.js"),
    ("internal/deps/acorn/acorn-walk/dist/walk", "deps/acorn/acorn-walk/dist/walk.js"),
    ("internal/deps/minimatch/index", "deps/minimatch/index.js"),
    ("internal/deps/undici/undici", "deps/undici/undici.js"),
];
