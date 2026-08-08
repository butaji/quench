const __quenchOriginalRequireWithGlob = globalThis.require;
const __quenchGlobEscape = (value) =>
  value.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&");
const __quenchGlobPattern = (pattern) => {
  let source = "";
  for (let index = 0; index < pattern.length;) {
    if (pattern.startsWith("**", index)) {
      if (pattern[index - 1] === "/" && index + 2 === pattern.length) {
        source = source.slice(0, -1) + "(?:/.*)?";
        index += 2;
        continue;
      }
      if (pattern[index + 2] === "/") {
        source += "(?:.*/)?";
        index += 3;
      } else {
        source += ".*";
        index += 2;
      }
    } else if (pattern[index] === "*") {
      source += "[^/]*";
      index++;
    } else if (pattern[index] === "?") {
      source += "[^/]";
      index++;
    } else if (
      pattern.startsWith("!(", index) &&
      pattern.indexOf(")", index) > index
    ) {
      const end = pattern.indexOf(")", index);
      const alternatives = pattern.slice(index + 2, end).split("|");
      source += `(?!${alternatives
        .map(__quenchGlobPattern)
        .join("|")}(?:/|$))[^/]*`;
      index = end + 1;
    } else if (pattern[index] === "[" && pattern.indexOf("]", index) > index) {
      const end = pattern.indexOf("]", index);
      source += pattern.slice(index, end + 1);
      index = end + 1;
    } else if (pattern[index] === "{" && pattern.indexOf("}", index) > index) {
      const end = pattern.indexOf("}", index);
      const alternatives = pattern.slice(index + 1, end).split(",");
      source += `(?:${alternatives.map(__quenchGlobPattern).join("|")})`;
      index = end + 1;
    } else if (
      pattern[index] === "+" &&
      pattern[index + 1] === "(" &&
      pattern.indexOf(")", index) > index
    ) {
      const end = pattern.indexOf(")", index);
      const alternatives = pattern.slice(index + 2, end).split("|");
      source += `(?:${alternatives.map(__quenchGlobPattern).join("|")})+`;
      index = end + 1;
    } else {
      source += __quenchGlobEscape(pattern[index++]);
    }
  }
  return source;
};
const __quenchGlobHiddenAllowed = (pattern, path) => {
  const patternParts = pattern.split("/");
  return path
    .split("/")
    .every(
      (part, index) =>
        !part.startsWith(".") ||
        patternParts[index]?.startsWith(".") ||
        patternParts[index] === "**"
    );
};
const __quenchGlobMatches = (pattern, path) => {
  const normalized = String(pattern).replace(/^\.\//, "").replace(/\/$/, "");
  const expression = new RegExp(`^${__quenchGlobPattern(normalized)}$`);
  return (
    expression.test(path) ||
    (normalized.endsWith("/**") &&
      new RegExp(
        `^${__quenchGlobPattern(normalized.slice(0, -3))}(?:/.*)?$`
      ).test(path))
  );
};
const __quenchGlobEntries = (pattern, options = {}) => {
  const cwdOption = options.cwd;
  const cwdText = cwdOption?.href?.startsWith("file:")
    ? decodeURIComponent(cwdOption.pathname)
    : String(cwdOption || globalThis.__quench_cwd || ".");
  const cwd = cwdText.replace(/\\/g, "/").replace(/\/$/, "") || "/";
  const input = String(pattern).replace(/\\/g, "/");
  const absolute = input.startsWith("/");
  let normalized = input.replace(/^\.\//, "").replace(/\/+$|^$/, "");
  while (normalized.includes("/../")) {
    normalized = normalized.replace(/\/[^/]+\/\.\.\/([^/]+)/, "/$1");
  }
  const expression = new RegExp(`^${__quenchGlobPattern(normalized)}$`);
  const recurse = normalized.includes("/");
  const result = [];
  const visited = new Set();
  const visit = (directory, relative) => {
    if (options.follow || options.followSymbolicLinks) {
      let identity = directory;
      try {
        identity = globalThis.__nodeFs.realpathSync(directory);
      } catch {}
      if (visited.has(identity)) return;
      visited.add(identity);
    }
    let entries;
    try {
      entries = globalThis.__nodeFs.readdirSync(directory, {
        withFileTypes: true
      });
    } catch {
      return;
    }
    for (const entry of entries) {
      const childRelative = relative ? `${relative}/${entry.name}` : entry.name;
      const childPath = `${directory}/${entry.name}`;
      const excluded =
        typeof options.exclude === "function"
          ? options.exclude(entry)
          : Array.isArray(options.exclude) &&
            options.exclude.some(
              (item) =>
                __quenchGlobMatches(item, childRelative) ||
                (String(item).startsWith("/") &&
                  __quenchGlobMatches(item, childPath))
            );
      if (
        !excluded &&
        expression.test(childRelative) &&
        __quenchGlobHiddenAllowed(normalized, childRelative)
      ) {
        result.push({ entry, relative: childRelative, path: childPath });
      }
      if (
        recurse &&
        (entry.isDirectory?.() ||
          ((options.follow || options.followSymbolicLinks) &&
            entry.isSymbolicLink?.()))
      ) {
        visit(childPath, childRelative);
      }
    }
  };
  visit(cwd, "");
  result.sort((left, right) => left.relative.localeCompare(right.relative));
  return result.map(({ entry, relative, path }) => ({
    entry,
    value: options.withFileTypes === true ? entry : absolute ? path : relative
  }));
};
const __quenchGlob = async function* (pattern, options = {}) {
  for (const result of __quenchGlobEntries(pattern, options)) {
    yield result.value;
  }
};
const __quenchGlobSync = (pattern, options = {}) =>
  __quenchGlobEntries(pattern, options).map((result) => result.value);
globalThis.__nodeFs.glob = __quenchGlob;
globalThis.__nodeFs.globSync = __quenchGlobSync;
globalThis.__nodeFs.promises.glob = __quenchGlob;
globalThis.__nodeFs.promises.globSync = __quenchGlobSync;
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  if (name === "fs/promises") {
    return Object.assign({}, __quenchOriginalRequireWithGlob(specifier), {
      glob: __quenchGlob,
      globSync: __quenchGlobSync
    });
  }
  if (name === "fs") {
    const module = __quenchOriginalRequireWithGlob(specifier);
    module.promises.glob = __quenchGlob;
    module.glob = __quenchGlob;
    module.globSync = __quenchGlobSync;
    return module;
  }
  return __quenchOriginalRequireWithGlob(specifier);
};
