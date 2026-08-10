//! Polyfill: `args`

pub const JS: &str = r#"const __quenchOriginalRequireWithParseArgs = globalThis.require;
const __quenchParseOption = (
  argument,
  args,
  index,
  definitions,
  values,
  tokens,
) => {
  const negative = argument.startsWith("--no-");
  const name = (
    negative ? argument.slice(5) : argument.replace(/^-+/, "")
  ).split("=")[0];
  const definition = definitions[name] || {};
  const separator = argument.indexOf("=");
  const inline = separator >= 0 ? argument.slice(separator + 1) : undefined;
  const value = definition.type === "boolean"
    ? !negative
    : (inline ?? args[index + 1]);
  values[name] = definition.multiple ? [...(values[name] || []), value] : value;
  tokens.push({ kind: "option", name, value });
  return separator >= 0 || definition.type === "boolean" ? index : index + 1;
};
const __quenchParseArgs = (options = {}) => {
  const args = options.args || [];
  const definitions = options.options || {};
  const values = {},
    positionals = [],
    tokens = [];
  for (let index = 0; index < args.length; index++) {
    const argument = args[index];
    if (!argument.startsWith("-")) {
      positionals.push(argument);
      tokens.push({ kind: "positional", value: argument });
      continue;
    }
    index = __quenchParseOption(
      argument,
      args,
      index,
      definitions,
      values,
      tokens,
    );
  }
  return { values, positionals, ...(options.tokens ? { tokens } : {}) };
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "util") {
    return Object.assign({}, __quenchOriginalRequireWithParseArgs(specifier), {
      parseArgs: __quenchParseArgs,
    });
  }
  return __quenchOriginalRequireWithParseArgs(specifier);
};
"#;
