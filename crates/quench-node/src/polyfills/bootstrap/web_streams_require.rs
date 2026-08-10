//! Polyfill: `web-streams-require`

pub const JS: &str = r#"globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  if (name === "internal/webstreams/util") {
    return { kState: __quenchWebStreamsState };
  }
  if (name === "stream/web") return __quenchWebStreams;
  return __quenchOriginalRequireWithWebStreams(specifier);
};
"#;
