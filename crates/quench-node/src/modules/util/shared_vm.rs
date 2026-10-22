//! Shared-VM utilities whose behavior is defined in guest-visible JavaScript.

use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};

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

  const propertyName = (key) => /^[A-Za-z_$][\\w$]*$/.test(key) ? key : quote(key);

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
        if (typeof current.stack === "string") return current.stack;
        return `${current.name || "Error"}${current.message ? `: ${current.message}` : ""}`;
      }
      if (depth > maxDepth) return Array.isArray(current) ? "[Array]" : "[Object]";

      const customInspect = current[inspect.custom];
      if (settings.customInspect !== false && typeof customInspect === "function") {
        const custom = Reflect.apply(customInspect, current, [depth, settings, inspect]);
        if (typeof custom === "string") return custom;
        if (custom !== current) return render(custom, depth + 1);
      }
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
  const utf8Bytes = (value) => {
    const bytes = [];
    for (const character of String(value)) {
      let code = character.codePointAt(0);
      if (code >= 0xd800 && code <= 0xdfff) code = 0xfffd;
      if (code <= 0x7f) bytes.push(code);
      else if (code <= 0x7ff) {
        bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
      } else if (code <= 0xffff) {
        bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
      } else {
        bytes.push(
          0xf0 | (code >> 18),
          0x80 | ((code >> 12) & 0x3f),
          0x80 | ((code >> 6) & 0x3f),
          0x80 | (code & 0x3f),
        );
      }
    }
    return bytes;
  };
  class TextEncoder {
    get encoding() {
      return "utf-8";
    }
    encode(value = "") {
      return Uint8Array.from(utf8Bytes(value));
    }
    encodeInto(value, destination) {
      const source = String(value);
      let read = 0;
      let written = 0;
      for (const character of source) {
        const width = character.length;
        const bytes = utf8Bytes(character);
        if (written + bytes.length > destination.length) break;
        for (const byte of bytes) destination[written++] = byte;
        read += width;
      }
      return { read, written };
    }
  }
  class TextDecoder {
    constructor(label = "utf-8") {
      if (!/^utf-?8$/i.test(String(label))) {
        throw new RangeError(`The "${label}" encoding is not supported`);
      }
      this.encoding = "utf-8";
      this.fatal = false;
      this.ignoreBOM = false;
    }
    decode(input = new Uint8Array()) {
      const bytes = input instanceof ArrayBuffer
        ? new Uint8Array(input)
        : new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
      let binary = "";
      for (let offset = 0; offset < bytes.length; offset += 0x4000) {
        binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x4000));
      }
      try {
        const text = decodeURIComponent(escape(binary));
        return this.ignoreBOM ? text : text.replace(/^\uFEFF/, "");
      } catch {
        if (this.fatal) throw new TypeError("The encoded data was not valid UTF-8");
        return "\uFFFD";
      }
    }
  }
  return {
    inspect,
    getCallSites,
    inherits,
    TextEncoder,
    TextDecoder,
  };
})()"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(UTIL, "node:util/shared.js")
}
