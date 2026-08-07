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
};
const __quenchStyleText = (text, style, options = {}) => {
  if (options.validateStream === false || options.colors === false) {
    return String(text);
  }
  const styles = Array.isArray(style) ? style : [style];
  return styles.reduce((value, name) => {
    const codes = __quenchStyles[name];
    return codes ? `\u001b[${codes[0]}m${value}\u001b[${codes[1]}m` : value;
  }, String(text));
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "util") {
    return Object.assign({}, __quenchOriginalRequireWithStyleText(specifier), {
      styleText: __quenchStyleText,
    });
  }
  return __quenchOriginalRequireWithStyleText(specifier);
};
