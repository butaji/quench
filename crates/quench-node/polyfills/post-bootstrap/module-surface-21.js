{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "vm") {
        result = Object.assign({}, result);
        result.runInContext ||= () => undefined;
        result.runInNewContext ||= () => undefined;
        result.runInThisContext ||= () => undefined;
        result.createContext ||= () => ({});
        result.isContext ||= () => false;
        result.compileFunction ||= () => () => undefined;
        for (
          const constructor of [
            "Script",
            "Context",
            "Module",
            "SourceTextModule",
            "SyntheticModule",
          ]
        ) {
          result[constructor] ||= function Constructor() {};
        }
      }
      return result;
    };
  }
}
