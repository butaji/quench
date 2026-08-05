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
globalThis.__quenchResolveParsedWebAbsolute = (r, f, t) => {
  const source = f.href || f.pathname || "";
  if (!t.startsWith("/") || !source.includes("://")) return null;
  return r.parse(globalThis.__quenchResolveAbsolutePath(source, t));
};
globalThis.__quenchResolveParsedWebRelative = (r, f, t) => {
  const source = f.href || "";
  const origin = source.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0];
  if (!origin || t.startsWith("/")) return null;
  const path = source.slice(origin.length).split(/[?#]/)[0] || "/";
  if (/^[?#]/.test(t)) return r.parse(`${origin}${path}${t}`);
  const base = path.slice(0, path.lastIndexOf("/") + 1);
  const normalized = globalThis.__quenchNormalizeAbsoluteTarget(`${base}${t}`);
  return r.parse(`${origin}${normalized}${t.endsWith("/") ? "/" : ""}`);
};
