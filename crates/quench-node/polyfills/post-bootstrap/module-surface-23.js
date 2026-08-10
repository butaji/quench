{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const normalized = String(name).replace(/^node:/, "");
      if (normalized === "trace_events") {
        return {
          createTracing: (options) => {
            let enabled = Boolean(options?.enabled);
            return {
              get enabled() {
                return enabled;
              },
              enable: () => {
                enabled = true;
              },
              disable: () => {
                enabled = false;
              },
              categories: (options?.categories || []).join(","),
            };
          },
          getEnabledCategories: () => "",
        };
      }
      return originalRequire(name);
    };
  }
}
