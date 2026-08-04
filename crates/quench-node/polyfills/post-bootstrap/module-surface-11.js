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
