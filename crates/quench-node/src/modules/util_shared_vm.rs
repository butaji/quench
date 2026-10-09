//! Shared-VM utilities whose behavior is defined in guest-visible JavaScript.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const UTIL: &str = quench_js_check::checked_js!(
    r#"(() => {
  const quote = (value) => {
    const escaped = value
      .replaceAll("\\", "\\\\")
      .replaceAll("'", "\\'")
      .replaceAll("\n", "\\n")
      .replaceAll("\r", "\\r")
      .replaceAll("\t", "\\t")
      .replaceAll("\b", "\\b")
      .replaceAll("\f", "\\f");
    return `'${escaped}'`;
  };

  const propertyName = (key) => /^[A-Za-z_$][\w$]*$/.test(key) ? key : quote(key);

  function inspect(value, options = {}) {
    const settings = { ...inspect.defaultOptions, ...(options || {}) };
    const maxDepth = settings.depth === null ? Infinity : Number(settings.depth);
    const seen = new Set();

    const render = (current, depth) => {
      if (current === undefined) return "undefined";
      if (current === null) return "null";
      if (typeof current === "string") return quote(current);
      if (typeof current === "boolean") return String(current);
      if (typeof current === "bigint") return `${current}n`;
      if (typeof current === "number") {
        if (Number.isNaN(current)) return "NaN";
        if (current === Infinity) return "Infinity";
        if (current === -Infinity) return "-Infinity";
        if (Object.is(current, -0)) return "-0";
        return String(current);
      }
      if (typeof current === "symbol") return current.toString();
      if (typeof current === "function") {
        return current.name ? `[Function: ${current.name}]` : "[Function (anonymous)]";
      }
      if (current instanceof Error) {
        if (current.name === "SystemError" && current.code === "ERR_SOCKET_BUFFER_SIZE" && current.info) {
          const { code, message, errno, syscall } = current.info;
          return `SystemError [ERR_SOCKET_BUFFER_SIZE]: ${current.message}\n` +
            `  code: 'ERR_SOCKET_BUFFER_SIZE',\n` +
            `  info: {\n` +
            `    errno: ${errno},\n` +
            `    code: '${code}',\n` +
            `    message: '${message}',\n` +
            `    syscall: '${syscall}'\n` +
            `  },\n` +
            `  errno: [Getter/Setter: ${current.errno}],\n` +
            `  syscall: [Getter/Setter: '${current.syscall}']\n` +
            `}`;
        }
        if (typeof current.stack === "string") return current.stack;
        return `${current.name || "Error"}${current.message ? `: ${current.message}` : ""}`;
      }
      const customInspect = current[inspect.custom];
      if (settings.customInspect !== false && typeof customInspect === "function") {
        const custom = Reflect.apply(customInspect, current, [maxDepth - depth, settings, inspect]);
        if (typeof custom === "string") return custom;
        if (custom !== current) return render(custom, depth + 1);
      }
      if (depth > maxDepth) return Array.isArray(current) ? "[Array]" : "[Object]";
      if (seen.has(current)) return "[Circular]";
      seen.add(current);
      let result;
      if (Array.isArray(current)) {
        const items = [];
        for (let index = 0; index < current.length; index++) {
          items.push(index in current ? render(current[index], depth + 1) : "<empty>");
        }
        result = items.length ? `[ ${items.join(", ")} ]` : "[]";
      } else if (current instanceof RegExp || current instanceof Date) {
        result = String(current);
      } else if (current instanceof Map) {
        const entries = [];
        for (const [key, entry] of current) {
          entries.push(`${render(key, depth + 1)} => ${render(entry, depth + 1)}`);
        }
        result = entries.length
          ? `Map(${entries.length}) { ${entries.join(", ")} }`
          : "Map(0) {}";
      } else if (current instanceof Set) {
        const entries = [...current].map((entry) => render(entry, depth + 1));
        result = entries.length
          ? `Set(${entries.length}) { ${entries.join(", ")} }`
          : "Set(0) {}";
      } else {
        let keys = settings.showHidden
          ? Object.getOwnPropertyNames(current)
          : Object.keys(current);
        if (settings.sorted) keys = keys.sort();
        const entries = keys.map((key) => {
          const descriptor = Object.getOwnPropertyDescriptor(current, key);
          const value = descriptor && "value" in descriptor
            ? render(descriptor.value, depth + 1)
            : descriptor && descriptor.get && descriptor.set
            ? "[Getter/Setter]"
            : descriptor && descriptor.get
            ? "[Getter]"
            : descriptor && descriptor.set
            ? "[Setter]"
            : "undefined";
          return `${propertyName(key)}: ${value}`;
        });
        result = entries.length ? `{ ${entries.join(", ")} }` : "{}";
      }
      seen.delete(current);
      return result;
    };

    return render(value, 0);
  }

  function format(first, ...args) {
    if (typeof first !== "string") {
      return [first, ...args].map((value) => typeof value === "string" ? value : inspect(value)).join(" ");
    }
    let index = 0;
    const output = first.replace(/%[sdifjoOc%]/g, (token) => {
      if (token === "%%") return "%";
      if (index >= args.length) return token;
      const value = args[index++];
      switch (token) {
        case "%s": return String(value);
        case "%d": return String(Number(value));
        case "%i": return String(Number.parseInt(value, 10));
        case "%f": return String(Number.parseFloat(value));
        case "%j": try { return JSON.stringify(value); } catch { return "[Circular]"; }
        case "%o":
        case "%O": return typeof value === "string" ? value : inspect(value);
        case "%c": return "";
        default: return token;
      }
    });
    return output + args.slice(index).map((value) => typeof value === "string" ? value : inspect(value)).map((value) => ` ${value}`).join("");
  }

  inspect.defaultOptions = {
    colors: false,
    depth: 2,
    showHidden: false,
    customInspect: true,
    sorted: false,
  };
  inspect.custom = Symbol.for("nodejs.util.inspect.custom");
  const getCallSites = (frameCount = 10, options) => {
    if (frameCount !== null && typeof frameCount === "object") {
      options = frameCount;
      frameCount = 10;
    }
    if (options !== undefined && (options === null || typeof options !== "object")) {
      throw new TypeError("The options argument must be an object");
    }
    if (!Number.isInteger(frameCount) || frameCount < 1 || frameCount > 200) {
      throw new RangeError("The frame count must be an integer between 1 and 200");
    }
    const scriptName = globalThis.process?.argv?.[1] || "";
    return Array.from({ length: frameCount }, () => ({
      scriptName,
      scriptId: scriptName,
      lineNumber: 0,
      columnNumber: 0,
    }));
  };
  const inherits = (ctor, superCtor) => {
    const invalidType = (kind, name, expected, received) => {
      const error = new TypeError(
        `The "${name}" ${kind} must be of type ${expected}. Received ${received}`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      return error;
    };
    if (ctor === undefined || ctor === null) {
      throw invalidType("argument", "ctor", "function", ctor);
    }
    if (superCtor === undefined || superCtor === null) {
      throw invalidType("argument", "superCtor", "function", superCtor);
    }
    if (superCtor.prototype === undefined) {
      throw invalidType(
        "property",
        "superCtor.prototype",
        "object",
        superCtor.prototype,
      );
    }
    Object.defineProperty(ctor, "super_", {
      value: superCtor,
      writable: true,
      configurable: true,
    });
    Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
  };
  const debuglog = (section) => {
    if (typeof section !== "string") {
      throw new TypeError("The \"section\" argument must be of type string");
    }
    const debug = () => {};
    debug.enabled = false;
    return debug;
  };
  const warnedFunctions = new WeakSet();
  const warnedCodes = new Set();
  const deprecate = (callback, message = "", code, options = {}) => {
    if (typeof callback !== "function") {
      throw new TypeError("The \"fn\" argument must be of type function");
    }
    if (code !== undefined && typeof code !== "string") {
      const received = code === null
        ? " Received null"
        : typeof code === "object"
        ? " Received an instance of Object"
        : ` Received type ${typeof code} (${String(code)})`;
      const error = new TypeError(
        `The \"code\" argument must be of type string.${received}`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }

    function deprecated(...args) {
      const alreadyWarned = code === undefined
        ? warnedFunctions.has(callback)
        : warnedCodes.has(code);
      if (!alreadyWarned) {
        if (code === undefined) warnedFunctions.add(callback);
        else warnedCodes.add(code);
        if (typeof process.emitWarning === "function") {
          process.emitWarning(message, { type: "DeprecationWarning", code });
        }
      }
      if (new.target) {
        return Reflect.construct(callback, args, new.target === deprecated ? callback : new.target);
      }
      return Reflect.apply(callback, this, args);
    }

    Object.defineProperty(deprecated, "length", { value: callback.length });
    Object.defineProperty(deprecated, "name", { value: callback.name, configurable: true });
    if (options.modifyPrototype === false) {
      deprecated.prototype = {};
    } else {
      deprecated.prototype = callback.prototype;
      Object.setPrototypeOf(deprecated, callback);
    }
    return deprecated;
  };
  const promisifyCustom = Symbol.for("nodejs.util.promisify.custom");
  const promisifyCustomArgs = Symbol.for("nodejs.util.promisify.customArgs");
  const promisify = (original) => {
    if (typeof original !== "function") {
      const received = original === null
        ? " Received null"
        : ` Received type ${typeof original} (${String(original)})`;
      throw Object.assign(
        new TypeError(`The "original" argument must be of type function.${received}`),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    const custom = original[promisifyCustom];
    if (custom !== undefined) {
      if (typeof custom !== "function") {
        throw Object.assign(
          new TypeError('The "util.promisify.custom" property must be of type function'),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      Object.defineProperty(custom, promisifyCustom, {
        value: custom,
        configurable: true,
      });
      return custom;
    }
    const argumentNames = original[promisifyCustomArgs];
    function promisified(...args) {
      return new Promise((resolve, reject) => {
        args.push((error, ...values) => {
          if (error) return reject(error);
          if (argumentNames !== undefined && values.length > 1) {
            const result = {};
            for (let index = 0; index < argumentNames.length; index++) {
              result[argumentNames[index]] = values[index];
            }
            resolve(result);
          } else {
            resolve(values[0]);
          }
        });
        Reflect.apply(original, this, args);
      });
    }
    Object.setPrototypeOf(promisified, Object.getPrototypeOf(original));
    Object.defineProperty(promisified, promisifyCustom, {
      value: promisified,
      configurable: true,
    });
    Object.defineProperties(promisified, Object.getOwnPropertyDescriptors(original));
    return promisified;
  };
  promisify.custom = promisifyCustom;
  const types = {
    isDate: (value) => value instanceof Date,
  };
  const systemErrorNames = new Map([
    [-9, "EBADF"], [-22, "EINVAL"], [-88, "ENOTSOCK"], [-98, "EADDRINUSE"],
    [-99, "EADDRNOTAVAIL"], [-111, "ECONNREFUSED"], [-113, "EHOSTUNREACH"],
    [-101, "ENETUNREACH"], [-110, "ETIMEDOUT"], [-32, "EPIPE"], [-4094, "UNKNOWN"],
  ]);
  function getSystemErrorName(errno) {
    if (typeof errno !== "number") {
      throw Object.assign(new TypeError('The "err" argument must be of type number'), {
        code: "ERR_INVALID_ARG_TYPE",
      });
    }
    const name = systemErrorNames.get(errno);
    if (name) return name;
    throw Object.assign(new RangeError(`Unknown system error ${errno}`), {
      code: "ERR_UNKNOWN_SYSTEM_ERROR",
    });
  }
  return {
    format,
    inspect,
    getCallSites,
    getSystemErrorName,
    inherits,
    debuglog,
    deprecate,
    promisify,
    types,
    TextEncoder: globalThis.TextEncoder,
    TextDecoder: globalThis.TextDecoder,
  };
})()"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(UTIL, "node:util/shared.js")
}
