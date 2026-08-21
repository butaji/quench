//! Polyfill: `filesystem-constants`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithFsConstants = globalThis.require;
const __quenchFsConstants = Object.freeze({
  F_OK: 0,
  R_OK: 4,
  W_OK: 2,
  X_OK: 1,
  O_RDONLY: 0,
  O_WRONLY: 1,
  O_RDWR: 2,
  O_CREAT: 64,
  O_EXCL: 128,
  O_TRUNC: 512,
  O_APPEND: 1024,
  COPYFILE_EXCL: 1,
  COPYFILE_FICLONE: 2,
  COPYFILE_FICLONE_FORCE: 4,
  S_IRUSR: 0o400,
  S_IWUSR: 0o200,
  S_IXUSR: 0o100,
  S_IFDIR: 0o40000,
});
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "fs") {
    const module = __quenchOriginalRequireWithFsConstants(specifier);
    module.constants = Object.freeze(
      new Proxy(
        Object.assign(
          Object.create(null),
          module.constants,
          __quenchFsConstants,
        ),
        { getPrototypeOf: () => null },
      ),
    );
    return module;
  }
  return __quenchOriginalRequireWithFsConstants(specifier);
};
"#);
