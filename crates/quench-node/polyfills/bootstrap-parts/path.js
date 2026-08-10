const __nodePathFormatExtension = (extension) => {
  if (!extension) return "";
  const value = String(extension);
  return value.startsWith(".") ? value : `.${value}`;
};
const __nodePathFormatParts = (parts) => {
  if (!parts || typeof parts !== "object" || Array.isArray(parts)) {
    const error = new TypeError(
      'The "pathObject" argument must be of type object.' +
        (globalThis.__nodeCommon?.invalidArgTypeHelper?.(parts) || ""),
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __nodeWindowsRoot = (input) => {
  if (input.startsWith("\\\\")) {
    const parts = input.split("\\");
    return parts.length >= 4 && parts[2] && parts[3]
      ? `\\\\${parts[2]}\\${parts[3]}\\`
      : "\\";
  }
  if (/^[A-Za-z]:\\/.test(input)) return input.slice(0, 3);
  if (/^[A-Za-z]:/.test(input)) return input.slice(0, 2);
  return input.startsWith("\\") ? "\\" : "";
};
const __nodeWindowsParts = (base, root, dir) => {
  const dot = base.lastIndexOf(".");
  const ext = dot > 0 ? base.slice(dot) : "";
  return {
    root,
    dir,
    base,
    ext,
    name: ext ? base.slice(0, -ext.length) : base,
  };
};
const __nodeWindowsKeepSeparator = (trimmed, root, index, trailing) => {
  if (trailing) return true;
  if (index > 0 && trimmed[index - 1] === "\\" && trimmed[index - 2] !== "\\") {
    return true;
  }
  if (root.length === 3 && index === 2) return true;
  return root.startsWith("\\\\") && index === root.length - 1;
};
const __nodeWindowsDir = (trimmed, root, index, trailing) => {
  if (index < 0) return "";
  const keepSeparator = __nodeWindowsKeepSeparator(
    trimmed,
    root,
    index,
    trailing,
  );
  return trimmed.slice(0, index + (keepSeparator ? 1 : 0)) || root;
};
const __nodeGlobNormalize = (value) =>
  value.replace(/\\/g, "/").replace(/\/+/g, "/").replace(/\/$/, "") || ".";
const __nodeGlobSegment = (value, pattern) => {
  if (pattern.includes("{") && pattern.includes("}")) {
    const open = pattern.indexOf("{");
    const close = pattern.indexOf("}", open);
    const head = pattern.slice(0, open);
    const tail = pattern.slice(close + 1);
    return pattern
      .slice(open + 1, close)
      .split(",")
      .some((part) => __nodeGlobSegment(value, head + part + tail));
  }
  const expression = pattern
    .replace(/[.+^$()|\\]/g, "\\$&")
    .replace(/\{[^}]*\}/g, (match) => match)
    .replace(/\*\*/g, "::DOUBLESTAR::")
    .replace(/\*/g, "[^/]*")
    .replace(/::DOUBLESTAR::/g, ".*")
    .replace(/\?/g, "[^/]")
    .replace(/\[!([^\]]+)\]/g, "[^$1]")
    .replace(/\[([^\]]+)\]/g, "[$1]");
  try {
    return new RegExp("^" + expression + "$").test(value);
  } catch {
    return false;
  }
};
const __nodeGlobMatch = (path, pattern) => {
  if (typeof path !== "string" || typeof pattern !== "string") {
    const error = new TypeError(
      'The "path" and "pattern" arguments must be of type string',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const paths = __nodeGlobNormalize(path).split("/");
  const patterns = __nodeGlobNormalize(pattern).split("/");
  let pathIndex = 0;
  let patternIndex = 0;
  while (patternIndex < patterns.length) {
    const segment = patterns[patternIndex];
    if (segment === "**") {
      if (patternIndex === patterns.length - 1) return true;
      patternIndex++;
      for (let index = pathIndex; index <= paths.length; index++) {
        if (
          __nodeGlobMatch(
            paths.slice(index).join("/"),
            patterns.slice(patternIndex).join("/"),
          )
        ) {
          return true;
        }
      }
      return false;
    }
    if (
      pathIndex >= paths.length ||
      !__nodeGlobSegment(paths[pathIndex], segment)
    ) {
      return false;
    }
    pathIndex++;
    patternIndex++;
  }
  return pathIndex === paths.length;
};
const __nodeWindowsDriveParse = (input) => {
  if (/^[A-Za-z]:\\$/.test(input) || /^[A-Za-z]:$/.test(input)) {
    return { root: input, dir: input, base: "", ext: "", name: "" };
  }
  if (!/^[A-Za-z]:[^\\]/.test(input)) return null;
  const relative = input.slice(2);
  const dot = relative.lastIndexOf(".");
  const ext = dot > 0 ? relative.slice(dot) : "";
  return __nodeWindowsParts(relative, input.slice(0, 2), input.slice(0, 2));
};
const __nodeWindowsNamespacedPath = (value) => {
  if (typeof value !== "string") return value;
  if (value.startsWith("\\\\?\\")) return value.replace(/\//g, "\\");
  const input = value.replace(/\//g, "\\");
  if (input.startsWith("\\\\")) {
    return `\\\\?\\UNC\\${input.slice(2).replace(/\\+/g, "\\")}\\`;
  }
  if (/^[A-Za-z]:\\/.test(input)) return `\\\\?\\${input}`;
  return `\\\\?\\${__nodeWinPath.resolve(input)}`;
};
const __nodeWindowsDirnameKeep = (input, uncRoot, index) => {
  if (index > 0 && input[index - 1] === "\\") return true;
  if (/^[A-Za-z]:\\/.test(input) && index === 2) return true;
  return uncRoot.startsWith("\\\\") && index === uncRoot.length - 1;
};
const __nodeWindowsRelativeRoot = (input) => {
  if (!input.startsWith("\\\\")) return __nodeWindowsRoot(input);
  const server = input.split("\\")[2];
  return server ? `\\\\${server}` : "\\\\";
};
globalThis.__nodePath = {
  sep: "/",
  delimiter: ":",
  isAbsolute: (value) => __nodePathArg(value).startsWith("/"),
  resolve: (...parts) => {
    parts.forEach(__nodePathArg);
    let resolved = "";
    let trailing = false;
    for (const part of parts) {
      if (!part) continue;
      resolved = resolved
        ? resolved + "/" + part.replace(/^\/+/, "")
        : part.startsWith("/")
        ? part
        : globalThis.__quench_cwd_get() + "/" + part;
      trailing = part.endsWith("/") && !part.endsWith("\\");
    }
    const out = globalThis.__nodePath.normalize(resolved);
    return trailing && !out.endsWith("/") ? out + "/" : out;
  },
  normalize: (value) => {
    const input = __nodePathArg(value);
    const absolute = input.startsWith("/");
    const parts = input.split("/").filter((part) => part && part !== ".");
    const output = [];
    parts.forEach((part) => {
      if (
        part === ".." &&
        output.length &&
        output[output.length - 1] !== ".."
      ) {
        output.pop();
      } else if (part !== "..") output.push(part);
    });
    const result = (absolute ? "/" : "") + output.join("/");
    return result || (absolute ? "/" : ".");
  },
  basename: (value, suffix) => {
    const base = __nodePathArg(value).split("/").pop();
    if (suffix !== undefined) {
      const end = __nodePathArg(suffix);
      return end && base.endsWith(end) ? base.slice(0, -end.length) : base;
    }
    return base;
  },
  dirname: (value) => {
    const input = __nodePathArg(value).replace(/\/+$/, "") || "/";
    const parts = input.split("/");
    parts.pop();
    return parts.join("/") || (input.startsWith("/") ? "/" : ".");
  },
  extname: (value) => {
    const input = __nodePathArg(value).replace(/\/+$/, "");
    const name = globalThis.__nodePath.basename(input);
    if (name === "." || name === "..") return "";
    const i = name.lastIndexOf(".");
    return i > 0 ? name.slice(i) : "";
  },
  join: (...parts) =>
    globalThis.__nodePath.normalize(parts.map(__nodePathArg).join("/")),
  relative: (from, to) => {
    const a = globalThis.__nodePath
      .normalize(__nodePathArg(from))
      .split("/")
      .filter(Boolean);
    const b = globalThis.__nodePath
      .normalize(__nodePathArg(to))
      .split("/")
      .filter(Boolean);
    while (a.length && a[0] === b[0]) {
      a.shift();
      b.shift();
    }
    return [...a.map(() => ".."), ...b].join("/") || "";
  },
  parse: (value) => {
    if (typeof value !== "string") {
      const error = new TypeError('The "path" argument must be of type string');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const input = String(value);
    const trimmed = input.replace(/\/+$/, "") ||
      (input.startsWith("/") ? "/" : "");
    const base = globalThis.__nodePath.basename(trimmed);
    const dir = input.endsWith("/") && trimmed === "."
      ? "."
      : input.includes("/")
      ? globalThis.__nodePath.dirname(trimmed)
      : "";
    const ext = globalThis.__nodePath.extname(base);
    return {
      root: input.startsWith("/") ? "/" : "",
      dir,
      base,
      ext,
      name: ext ? base.slice(0, -ext.length) : base,
    };
  },
  format: (parts) => {
    __nodePathFormatParts(parts);
    const dir = parts.dir || parts.root || "";
    const extension = __nodePathFormatExtension(parts.ext);
    const base = parts.base || `${parts.name || ""}${extension}`;
    if (!dir) return base;
    if (dir === "/") return `/${base}`;
    return `${dir}/${base}`;
  },
  matchesGlob(path, pattern) {
    return __nodeGlobMatch(path, pattern);
  },
  toNamespacedPath(value) {
    return value;
  },
};
globalThis.__nodePath.posix = globalThis.__nodePath;
const __nodeWinPath = {
  sep: "\\",
  delimiter: ";",
  isAbsolute(value) {
    const input = __nodePathArg(value);
    return (
      /^[A-Za-z]:[\\/]/.test(input) ||
      input.startsWith("/") ||
      input.startsWith("\\")
    );
  },
  normalize(value) {
    const input = __nodePathArg(value).replace(/\//g, "\\");
    const root = /^[A-Za-z]:\\/.test(input) ? input.slice(0, 3) : "";
    const parts = input.slice(root.length).split("\\").filter(Boolean);
    const output = [];
    for (const part of parts) {
      if (
        part === ".." &&
        output.length &&
        output[output.length - 1] !== ".."
      ) {
        output.pop();
      } else if (part !== ".") output.push(part);
    }
    return `${root}${output.join("\\")}${output.length ? "" : root ? "" : "."}`;
  },
  join(...parts) {
    return __nodeWinPath.normalize(parts.map(__nodePathArg).join("\\"));
  },
  resolve(...parts) {
    return __nodeWinPath.normalize(parts.map(__nodePathArg).join("\\"));
  },
  relative(from, to) {
    const fromInput = __nodePathArg(from).replace(/\//g, "\\");
    const toInput = __nodePathArg(to).replace(/\//g, "\\");
    const fromPath = __nodeWinPath.normalize(fromInput);
    const toPath = __nodeWinPath.normalize(toInput);
    if (
      __nodeWindowsRelativeRoot(fromInput) !==
        __nodeWindowsRelativeRoot(toInput)
    ) {
      return toInput;
    }
    const a = fromPath.split("\\").filter(Boolean);
    const b = toPath.split("\\").filter(Boolean);
    while (a.length && b.length && a[0].toLowerCase() === b[0].toLowerCase()) {
      a.shift();
      b.shift();
    }
    return [...a.map(() => ".."), ...b].join("\\");
  },
  parse(value) {
    __nodePathArg(value);
    if (value.startsWith("/")) return globalThis.__nodePath.parse(value);
    const input = value.replace(/\//g, "\\");
    const root = __nodeWindowsRoot(input);
    const hadTrailingSeparator = input.length > 0 && /[\\]$/.test(input);
    const trimmed = input.replace(/[\\]+$/, "") || root;
    const drive = __nodeWindowsDriveParse(input);
    if (drive) return drive;
    if (root.startsWith("\\\\") && trimmed === root.slice(0, -1)) {
      return { root, dir: root, base: "", ext: "", name: "" };
    }
    const index = trimmed.lastIndexOf("\\");
    const dir = __nodeWindowsDir(trimmed, root, index, hadTrailingSeparator);
    const base = index >= 0 ? trimmed.slice(index + 1) : trimmed;
    return __nodeWindowsParts(base, root, dir);
  },
  format(parts) {
    __nodePathFormatParts(parts);
    const extension = __nodePathFormatExtension(parts.ext);
    const base = parts.base || `${parts.name || ""}${extension}`;
    const dir = parts.dir || parts.root || "";
    if (!dir) return base;
    if (/^[A-Za-z]:$/.test(dir)) return `${dir}${base}`;
    if (dir.endsWith("\\")) return `${dir}${base}`;
    return `${dir}\\${base}`;
  },
  basename: (value, suffix) => {
    const input = __nodePathArg(value);
    if (/^[A-Za-z]:$/.test(input) || /^[A-Za-z]:[\\/]+$/.test(input)) return "";
    if (/^[A-Za-z]:[^\\]/.test(input)) return input.slice(2);
    const base = input
      .replace(/[\\/]+$/, "")
      .split(/[\\/]/)
      .pop() || "";
    if (suffix !== undefined) {
      const end = __nodePathArg(suffix);
      return end && base.endsWith(end) ? base.slice(0, -end.length) : base;
    }
    return base;
  },
  dirname: (value) => {
    const original = __nodePathArg(value);
    if (/^[A-Za-z]:[\\]+$/.test(original)) return original;
    if (/^[A-Za-z]:$/.test(original) || /^[A-Za-z]:[^\\]/.test(original)) {
      return original.slice(0, 2);
    }
    const input = original.replace(/[\\/]+$/, "");
    if (!input) return original ? "\\" : ".";
    const index = input.lastIndexOf("\\");
    if (index < 0) return "";
    const uncRoot = __nodeWindowsRoot(input);
    const keepSeparator = __nodeWindowsDirnameKeep(input, uncRoot, index);
    return input.slice(0, index + (keepSeparator ? 1 : 0)) || "\\";
  },
  extname(value) {
    const input = __nodePathArg(value).replace(/[\\/]+$/, "");
    const base = __nodeWinPath.basename(input);
    if (base === "." || base === "..") return "";
    const index = base.lastIndexOf(".");
    return index > 0 ? base.slice(index) : "";
  },
  matchesGlob(path, pattern) {
    return __nodeGlobMatch(
      __nodePathArg(path).replace(/[\\/]/g, "/"),
      __nodePathArg(pattern).replace(/[\\/]/g, "/"),
    );
  },
  toNamespacedPath(value) {
    return __nodeWindowsNamespacedPath(value);
  },
};
__nodeWinPath.posix = globalThis.__nodePath;
__nodeWinPath.win32 = __nodeWinPath;
globalThis.__nodePath.win32 = __nodeWinPath;
