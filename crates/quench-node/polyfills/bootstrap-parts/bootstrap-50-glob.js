const __quenchOriginalRequireWithGlob = globalThis.require;
const __quenchGlob = async function* (pattern, options = {}) {
  const cwd = options.cwd || globalThis.__quench_cwd || ".";
  const slash = String(pattern).lastIndexOf("/");
  const directory =
    slash < 0 ? cwd : cwd + "/" + String(pattern).slice(0, slash);
  const mask = slash < 0 ? String(pattern) : String(pattern).slice(slash + 1);
  const expression = new RegExp(
    "^" + mask.split("*").join(".*").split("?").join(".") + "$"
  );
  for (const entry of globalThis.__nodeFs.readdirSync(directory))
    if (expression.test(entry))
      yield slash < 0
        ? directory + "/" + entry
        : String(pattern).slice(0, slash) + "/" + entry;
};
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  if (name === "fs/promises")
    return Object.assign({}, __quenchOriginalRequireWithGlob(specifier), {
      glob: __quenchGlob
    });
  if (name === "fs") {
    const module = __quenchOriginalRequireWithGlob(specifier);
    module.promises.glob = __quenchGlob;
    return module;
  }
  return __quenchOriginalRequireWithGlob(specifier);
};
