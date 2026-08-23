//! Polyfill: `module-surface-11`

pub const JS: &str = quench_js_check::checked_js!(r##"{
  if (globalThis.require) {
    const moduleApi = globalThis.require("module");
    moduleApi.Module._resolveFilename ||= (name) => String(name);
    moduleApi.builtinModules ||= [];
    if (!moduleApi.builtinModules.includes("test")) {
      moduleApi.builtinModules.push("test");
    }
    moduleApi.isBuiltin = (name) => {
      const value = String(name);
      return (
        moduleApi.builtinModules.includes(value.replace(/^node:/, "")) &&
        (value.startsWith("node:") || value !== "test")
      );
    };
  }
}
globalThis.__quenchResolveParsedOpaque = (r, f, t) => {
  const source = f.href || f.pathname || "";
  const match = source.match(/^([A-Za-z][A-Za-z0-9+.-]*):(.*)$/);
  const dot = t.match(/^([A-Za-z][A-Za-z0-9+.-]*):\.$/);
  if (!match || (dot && dot[1] !== match[1])) return null;
  if (dot) return r.parse(`${match[1]}:`);
  if (t.startsWith("/")) {
    return r.parse(
      globalThis.__quenchNormalizeOpaqueRelative(match[1], "/", t)
    );
  }
  if (/^[A-Za-z][A-Za-z0-9+.-]*:/.test(t)) return null;
  const base = match[2].slice(0, match[2].lastIndexOf("/") + 1);
  return r.parse(globalThis.__quenchNormalizeOpaqueRelative(match[1], base, t));
};
globalThis.__quenchResolveParsedTripleAbsolute = (r, f, t) => {
  if (!f.slashes || f.host || !t.startsWith("/")) return null;
  if (f.slashes && !f.host && t.startsWith("//")) {
    return r.parse(`${f.protocol}${t}`);
  }
  return r.parse(`${f.protocol}//${t}`);
};
globalThis.__quenchResolveParsedWebAbsolute = (r, f, t) => {
  const source = f.href || f.pathname || "";
  if (!t.startsWith("/")) return null;
  const triple = globalThis.__quenchResolveParsedTripleAbsolute(r, f, t);
  if (triple) return triple;
  if (!source.includes("://")) return null;
  return r.parse(globalThis.__quenchResolveAbsolutePath(source, t));
};
globalThis.__quenchResolveParsedTripleSlash = (r, f, t) => {
  if (f.slashes && !f.host && f.pathname && !t.startsWith("/")) {
    const base = f.pathname.slice(0, f.pathname.lastIndexOf("/") + 1);
    const parts = `${base}${t}`.split("/");
    const normalized = [];
    for (const part of parts) {
      if (part === ".") continue;
      if (part === ".." && normalized.length > 1) normalized.pop();
      else normalized.push(part);
    }
    return r.parse(`${f.protocol}//${normalized.join("/")}`);
  }
  return null;
};
globalThis.__quenchResolveParsedWebRelative = (r, f, t) => {
  const source = f.href || "";
  const triple = globalThis.__quenchResolveParsedTripleSlash(r, f, t);
  if (triple) return triple;
  const origin = source.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/\/?[^/]*/)?.[0];
  if (!origin || t.startsWith("/")) return null;
  if (t === "") return r.parse(source);
  const path = source.slice(origin.length).split(/[?#]/)[0] || "/";
  if (/^[?#]/.test(t)) return r.parse(`${origin}${path}${t}`);
  const parameterBase = globalThis.__quenchParsedParameterBase(path, t);
  const base = parameterBase || path.slice(0, path.lastIndexOf("/") + 1);
  const targetPath = t.split(/[?#]/)[0];
  const suffix = t.slice(targetPath.length);
  const normalized = globalThis.__quenchNormalizeAbsoluteTarget(
    `${base}${targetPath}`
  );
  const trailing = globalThis.__quenchParsedWebTrailing(targetPath, normalized);
  return r.parse(`${origin}${normalized}${trailing}${suffix}`);
};
globalThis.__quenchParsedWebTrailing = (target, normalized) =>
  (target.endsWith("/") || target === "." || target === "..") &&
  !normalized.endsWith("/")
    ? "/"
    : "";
globalThis.__quenchParsedParameterBase = (path, target) => {
  const parameter = path.indexOf(";");
  return parameter >= 0 &&
    path.slice(parameter).includes("/") &&
    !target.startsWith("../")
    ? `${path}/`
    : null;
};
globalThis.__quenchResolveParsedAbsoluteOpaque = (r, f, t) =>
  /^[A-Za-z][A-Za-z0-9+.-]*:[^/]/.test(t) &&
  !/^([A-Za-z][A-Za-z0-9+.-]*):[#.]/.test(t)
    ? r.parse(t)
    : null;
globalThis.__quenchResolveParsedSameSchemeOpaque = (r, f, t) => {
  const target = t.match(/^([A-Za-z][A-Za-z0-9+.-]*):([^/].*)$/);
  const source = f.href || "";
  if (!target || !source.startsWith(`${target[1]}://`)) return null;
  return globalThis.__quenchResolveParsedWebRelative(r, f, target[2]);
};
globalThis.__quenchResolveParsedEmptyScheme = (r, f, t) => {
  const source = f.href || "";
  const match = t.match(/^([A-Za-z][A-Za-z0-9+.-]*):$/);
  return match && source.startsWith(`${match[1]}:`) ? r.parse(source) : null;
};
globalThis.__quenchResolveParsedFileEmpty = (result, from, to) =>
  to === "" && from.pathname && from.protocol === "file:"
    ? result.parse(`file:${from.pathname}`)
    : null;
globalThis.__quenchResolveParsedFileFragment = (result, from, to) => {
  const source = from.href || from.pathname || "";
  if (
    !source.startsWith("file:") ||
    /^[A-Za-z][A-Za-z0-9+.-]*:/.test(to) ||
    to.startsWith("//")
  ) {
    return null;
  }
  if (to.startsWith("#")) {
    return result.parse(
      `${
        source.startsWith("file:///") ? `file:/${source.slice(8)}` : source
      }${to}`
    );
  }
  const path = globalThis.__quenchResolveParsedPath(
    from.pathname || source,
    to
  );
  const trailing = globalThis.__quenchParsedWebTrailing(to, path);
  return result.parse(`file:${path}${trailing}`);
};
globalThis.__quenchResolveParsedFile = (result, from, to) =>
  globalThis.__quenchResolveParsedFileEmpty(result, from, to) ||
  globalThis.__quenchResolveParsedFileFragment(result, from, to);
globalThis.__quenchResolveParsedHash = (result, from, to) => {
  const source = from.href || from.pathname || "";
  return to.startsWith("#") && source.includes("://")
    ? result.parse(`${source.replace(/#.*$/, "")}${to}`)
    : null;
};
globalThis.__quenchResolveParsedMailto = (result, from, to) => {
  const source = from.href || "";
  const base = to.startsWith("?")
    ? source.replace(/[?#].*$/, "")
    : source.replace(/#.*$/, "");
  return from.protocol === "mailto:" && /^[?#]/.test(to)
    ? result.parse(`${base}${to}`)
    : null;
};
globalThis.__quenchResolveParsedMailtoOrHash = (result, from, to) =>
  globalThis.__quenchResolveParsedMailto(result, from, to) ||
  globalThis.__quenchResolveParsedHash(result, from, to);
globalThis.__quenchResolveParsedEmptyOpaque = (result, from, to) => {
  const source = from.href || "";
  return to === "" && source.includes(":") && !source.includes("://")
    ? result.parse(source.replace(/#.*$/, ""))
    : null;
};
globalThis.__quenchResolveParsedTextSpecial = (result, from, to) =>
  globalThis.__quenchResolveParsedMailtoOrHash(result, from, to) ||
  globalThis.__quenchResolveParsedEmptyOpaque(result, from, to) ||
  globalThis.__quenchResolveParsedFragmentBase(result, from, to);
globalThis.__quenchResolveParsedFragmentBase = (result, from, to) => {
  if (from.protocol || !from.hash) return null;
  return result.parse(
    to === ""
      ? `${from.pathname}${from.hash}`
      : to.startsWith("#")
        ? `${from.pathname}${to}`
        : to
  );
};
globalThis.__quenchResolveParsedAbsoluteTarget = (result, from, to) => {
  if (/^[A-Za-z][A-Za-z0-9+.-]*:\/\//.test(to)) return result.parse(to);
  return to.startsWith("//") && from.protocol
    ? result.parse(`${from.protocol}${to}`)
    : null;
};
globalThis.__quenchFileUrlHost = (input) =>
  input?.host ||
  (typeof input === "string" ? input.match(/^file:\/\/([^/]+)/)?.[1] : null);
globalThis.__quenchValidateFileUrlHost = (input, options) => {
  const host = globalThis.__quenchFileUrlHost(input);
  if (
    (input?.protocol === "file:" || typeof input === "string") &&
    host &&
    options?.windows !== true
  ) {
    throw Object.assign(new TypeError("File URL host must be empty"), {
      code: "ERR_INVALID_FILE_URL_HOST"
    });
  }
};
globalThis.__quenchPreserveEmptyQuery = (parsed, input) => {
  globalThis.__quenchEncodeLegacyPath(parsed);
  if (!input.endsWith("?") || parsed.hash) return;
  Object.assign(parsed, {
    search: "?",
    path: `${parsed.pathname || ""}?`,
    href: `${parsed.href || ""}?`
  });
};
globalThis.__quenchEncodeLegacyPath = (parsed) => {
  if (parsed.pathname) {
    parsed.pathname = parsed.pathname.replace(/[" <>]/g, (value) =>
      encodeURIComponent(value)
    );
  }
};
globalThis.__quenchNormalizeSpecialUrlInput = (value) =>
  /^(?:https?|ftp|wss?):/i.test(value) &&
  !/^(?:https?|ftp|wss?):\/\//i.test(value)
    ? value
        .replace(/^((?:https?|ftp|wss?):)\/?(?!\/)/i, "$1//")
        .replace(/^[^?#]*/, (path) => path.replace(/\\/g, "/"))
    : value;
globalThis.__quenchWhatwgFormat = (input) =>
  input.protocol === "tel:"
    ? `${input.protocol}${input.pathname}`
    : input.searchParams &&
        input.href &&
        !Object.prototype.hasOwnProperty.call(input, "auth")
      ? input.href
      : null;
globalThis.__quenchSpecialUrlProtocol = (protocol) =>
  /^(?:http|https|ftp|file):$/.test(protocol);
globalThis.__quenchResolveStringFragmentOnly = (from, to) =>
  typeof from === "string"
    ? globalThis.__quenchResolveFragmentOnly(from, to)
    : null;
globalThis.__quenchResolveParsedFragment = (result, from, to) => {
  if (typeof from === "string") return null;
  const target = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):(#.*)$/);
  const source = from.href || from.pathname || "";
  if (to.startsWith("#") && source.startsWith("file:")) {
    return result.parse(`${source.replace(/^file:\/\//, "file:/")}${to}`);
  }
  if (!target || !source.startsWith(`${target[1]}://`)) return null;
  return result.parse(`${source.replace(/#.*$/, "")}${target[2]}`);
};
globalThis.__nodeUrlEncode = (value) =>
  encodeURIComponent(
    String(value)
      .replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/g, "\uFFFD")
      .replace(/(^|[^\uD800-\uDBFF])[\uDC00-\uDFFF]/g, "$1\uFFFD")
  ).replace(/~/g, "%7E");
globalThis.__nodeInvalidThis = () => {
  const error = new TypeError(
    'Value of "this" must be of type URLSearchParams'
  );
  error.code = "ERR_INVALID_THIS";
  throw error;
};
globalThis.__nodeURLFormEncode = (value) =>
  globalThis
    .__nodeUrlEncode(value)
    .replace(
      /[!'()]/g,
      (char) => `%${char.charCodeAt(0).toString(16).toUpperCase()}`
    );
globalThis.__nodeURLDecode = (value) => {
  try {
    return decodeURIComponent(value);
  } catch (_) {
    return value;
  }
};
globalThis.__nodeURLSearchString = (value) => {
  if (typeof value === "symbol" || String(value).startsWith("Symbol(")) {
    throw new TypeError("Cannot convert a Symbol value to a string");
  }
  return String(value);
};
const __nodeURLSearchParamsToString =
  globalThis.__nodeURLSearchParams.prototype.toString;
globalThis.__nodeURLSearchParams.prototype.toString =
  Object.getOwnPropertyDescriptor(
    {
      toString() {
        if (!(this instanceof globalThis.__nodeURLSearchParams)) {
          return globalThis.__nodeInvalidThis();
        }
        return __nodeURLSearchParamsToString.call(this);
      }
    },
    "toString"
  ).value;
globalThis.__nodeURLSearchParams.prototype.sort ||=
  Object.getOwnPropertyDescriptor(
    {
      sort() {
        if (!(this instanceof globalThis.__nodeURLSearchParams)) {
          return globalThis.__nodeInvalidThis();
        }
        this._pairs.sort(([left], [right]) => left.localeCompare(right));
      }
    },
    "sort"
  ).value;
for (const name of "append delete get getAll has set sort toString".split(
  " "
)) {
  const descriptor = Object.getOwnPropertyDescriptor(
    globalThis.__nodeURLSearchParams.prototype,
    name
  );
  Object.defineProperty(globalThis.__nodeURLSearchParams.prototype, name, {
    ...descriptor,
    enumerable: true
  });
}
for (const name of ["append", "delete", "get", "getAll", "has", "set"]) {
  const original = globalThis.__nodeURLSearchParams.prototype[name];
  const required = name === "append" || name === "set" ? 2 : 1;
  const method = Object.getOwnPropertyDescriptor(
    {
      [name](...args) {
        if (!(this instanceof globalThis.__nodeURLSearchParams)) {
          return globalThis.__nodeInvalidThis();
        }
        if (args.length < required) {
          const label = required === 2 ? '"name" and "value"' : '"name"';
          const error = new TypeError(
            `The ${label} argument${
              required === 2 ? "s" : ""
            } must be specified`
          );
          error.code = "ERR_MISSING_ARGS";
          throw error;
        }
        return original.apply(this, args);
      }
    },
    name
  ).value;
  Object.defineProperty(globalThis.__nodeURLSearchParams.prototype, name, {
    configurable: true,
    enumerable: true,
    value: method,
    writable: true
  });
}
const __nodeURLSearchIterator = (values) => {
  let index = 0;
  const iterator = Object.defineProperties(
    Object.create(__nodeURLSearchIteratorPrototype),
    {
      __values: { configurable: true, value: values },
      __index: { configurable: true, value: 0, writable: true }
    }
  );
  iterator.next = function next() {
    if (this !== iterator) {
      throw Object.assign(
        new TypeError(
          'Value of "this" must be of type URLSearchParamsIterator'
        ),
        { code: "ERR_INVALID_THIS" }
      );
    }
    return index < values.length
      ? ((iterator.__index = index + 1),
        { done: false, value: values[index++] })
      : { done: true, value: undefined };
  };
  return iterator;
};
const __nodeURLSearchIteratorPrototype = {
  [Symbol.iterator]() {
    return this;
  },
  [Symbol.for("nodejs.util.inspect.custom")](depth, options = {}) {
    const values = this.__values.slice(this.__index);
    const rendered = values.map((value) =>
      Array.isArray(value) ? `[ '${value[0]}', '${value[1]}' ]` : `'${value}'`
    );
    if (options.breakLength <= 1) {
      return `URLSearchParams Iterator {\n  ${rendered.join(",\n  ")} }`;
    }
    return `URLSearchParams Iterator { ${rendered.join(", ")} }`;
  }
};
Object.defineProperty(__nodeURLSearchIteratorPrototype, Symbol.toStringTag, {
  value: "URLSearchParams Iterator"
});
const __nodeURLSearchValidateThis = (value) => {
  if (!(value instanceof globalThis.__nodeURLSearchParams)) {
    return globalThis.__nodeInvalidThis();
  }
  return value;
};
const __nodeURLSearchEntries = Object.getOwnPropertyDescriptor(
  {
    entries() {
      return __nodeURLSearchIterator(__nodeURLSearchValidateThis(this)._pairs);
    }
  },
  "entries"
).value;
const __nodeURLSearchKeys = Object.getOwnPropertyDescriptor(
  {
    keys() {
      return __nodeURLSearchIterator(
        __nodeURLSearchValidateThis(this)._pairs.map(([key]) => key)
      );
    }
  },
  "keys"
).value;
const __nodeURLSearchValues = Object.getOwnPropertyDescriptor(
  {
    values() {
      return __nodeURLSearchIterator(
        __nodeURLSearchValidateThis(this)._pairs.map(([, value]) => value)
      );
    }
  },
  "values"
).value;
globalThis.__nodeURLSearchParams.prototype.entries = __nodeURLSearchEntries;
globalThis.__nodeURLSearchParams.prototype.keys = __nodeURLSearchKeys;
globalThis.__nodeURLSearchParams.prototype.values = __nodeURLSearchValues;
globalThis.__nodeURLSearchParams.prototype[Symbol.iterator] =
  __nodeURLSearchEntries;
globalThis.__nodeURLSearchParams.prototype.forEach =
  Object.getOwnPropertyDescriptor(
    {
      forEach(callback, thisArg) {
        if (typeof callback !== "function") {
          const error = new TypeError(
            "The callback argument must be a function"
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        this._pairs.forEach(([key, value]) =>
          callback.call(thisArg, value, key, this)
        );
      }
    },
    "forEach"
  ).value;
const __nodeURLSearchInspect = Object.getOwnPropertyDescriptor(
  {
    inspect(depth, options = {}) {
      if (!(this instanceof globalThis.__nodeURLSearchParams)) {
        return globalThis.__nodeInvalidThis();
      }
      if (depth < 0) return "[Object]";
      const values = this._pairs.map(
        ([key, value]) => `'${key}' => '${value}'`
      );
      if (!values.length) return "URLSearchParams {}";
      if (options.breakLength <= 1) {
        return `URLSearchParams {\n  ${values.join(",\n  ")} }`;
      }
      return `URLSearchParams { ${values.join(", ")} }`;
    }
  },
  "inspect"
).value;
Object.defineProperty(__nodeURLSearchInspect, "name", {
  value: "[nodejs.util.inspect.custom]"
});
globalThis.__nodeURLSearchParams.prototype[
  Symbol.for("nodejs.util.inspect.custom")
] = __nodeURLSearchInspect;
"##);
