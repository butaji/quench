{
  if (globalThis.require) {
    const moduleApi = globalThis.require("module");
    moduleApi.Module._resolveFilename ||= (name) => String(name);
    moduleApi.builtinModules ||= [];
    if (!moduleApi.builtinModules.includes("test"))
      moduleApi.builtinModules.push("test");
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
  if (t.startsWith("/"))
    return r.parse(
      globalThis.__quenchNormalizeOpaqueRelative(match[1], "/", t)
    );
  if (/^[A-Za-z][A-Za-z0-9+.-]*:/.test(t)) return null;
  const base = match[2].slice(0, match[2].lastIndexOf("/") + 1);
  return r.parse(globalThis.__quenchNormalizeOpaqueRelative(match[1], base, t));
};
