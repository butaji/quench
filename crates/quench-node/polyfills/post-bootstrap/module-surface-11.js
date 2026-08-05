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
  if (t === "") return r.parse(source);
  const path = source.slice(origin.length).split(/[?#]/)[0] || "/";
  if (/^[?#]/.test(t)) return r.parse(`${origin}${path}${t}`);
  const base = path.slice(0, path.lastIndexOf("/") + 1);
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
globalThis.__quenchResolveParsedAbsoluteOpaque = (r, f, t) =>
  /^[A-Za-z][A-Za-z0-9+.-]*:[^/]/.test(t) &&
  !/^([A-Za-z][A-Za-z0-9+.-]*):#/.test(t)
    ? r.parse(t)
    : null;
globalThis.__quenchResolveParsedEmptyScheme = (r, f, t) => {
  const source = f.href || "";
  const match = t.match(/^([A-Za-z][A-Za-z0-9+.-]*):$/);
  return match && source.startsWith(`${match[1]}:`) ? r.parse(source) : null;
};
