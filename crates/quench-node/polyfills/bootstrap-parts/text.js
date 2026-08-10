const __quenchOriginalRequireWithStyleText = globalThis.require;
const __quenchStyles = {
  reset: [0, 0],
  bold: [1, 22],
  dim: [2, 22],
  italic: [3, 23],
  underline: [4, 24],
  red: [31, 39],
  green: [32, 39],
  yellow: [33, 39],
  blue: [34, 39],
  magenta: [35, 39],
  cyan: [36, 39],
  white: [37, 39],
  gray: [90, 39],
  bgRed: [41, 49],
  bgGreen: [42, 49],
  bgYellow: [43, 49],
  bgBlue: [44, 49],
  bgMagenta: [45, 49],
  bgCyan: [46, 49],
  bgWhite: [47, 49],
  bgGray: [100, 49],
};
const __quenchStyleAliases = {
  grey: "gray",
  bgGrey: "bgGray",
  blackBright: "gray",
  faint: "dim",
};
const __quenchStyleText = (style, text, options = {}) => {
  if (typeof style !== "string" && !Array.isArray(style)) {
    throw Object.assign(new TypeError("The 'format' argument must be a string or an Array"), { code: "ERR_INVALID_ARG_VALUE" });
  }
  if (typeof text !== "string") {
    throw Object.assign(new TypeError("The 'text' argument must be of type string"), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (
    options.validateStream !== false &&
    options.stream !== undefined &&
    (!options.stream ||
      typeof options.stream !== "object" ||
      typeof options.stream.isTTY !== "boolean")
  ) {
    throw Object.assign(new TypeError("The 'stream' option must be a TTY stream"), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (options.validateStream === false || options.colors === false) {
    if (options.colors === false) return text;
  } else if (options.stream) {
    const env = globalThis.process?.env || {};
    const forced = env.FORCE_COLOR !== undefined;
    if (
      !forced &&
      (options.stream.isTTY !== true || env.NODE_DISABLE_COLORS || env.NO_COLOR)
    ) {
      return text;
    }
  }
  const styles = Array.isArray(style) ? style : [style];
  const codes = styles
    .map((name) => {
      const normalized = __quenchStyleAliases[name] || name;
      if (normalized === "none") return null;
      const styleCodes = __quenchStyles[normalized];
      if (!styleCodes) {
        throw Object.assign(new TypeError(`Unknown style: ${name}`), { code: "ERR_INVALID_ARG_VALUE" });
      }
      return styleCodes;
    })
    .filter(Boolean);
  if (codes.length === 0) return text;
  let nestedText = text;
  nestedText = nestedText.replace(
    /\u001b\[39m(?=(?:\u001b\[\d+m)*[^\u001b])/g,
    `\u001b[${codes[0][0]}m`,
  );
  nestedText = nestedText.replace(
    /\u001b\[(22|23|24)m(?=(?:\u001b\[\d+m)*[^\u001b])/g,
    `$&\u001b[${codes[0][0]}m`,
  );
  return `${codes.map(([open]) => `\u001b[${open}m`).join("")}${nestedText}${
    [
      ...codes,
    ]
      .reverse()
      .map(([, close]) => `\u001b[${close}m`)
      .join("")
  }`;
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "util") {
    return Object.assign({}, __quenchOriginalRequireWithStyleText(specifier), {
      styleText: __quenchStyleText,
    });
  }
  return __quenchOriginalRequireWithStyleText(specifier);
};
