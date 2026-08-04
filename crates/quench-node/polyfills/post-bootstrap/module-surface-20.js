{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "v8") {
        result = Object.assign({}, result);
        result.serialize ||= (value) => value;
        result.deserialize ||= (value) => value;
        result.getHeapStatistics ||= () => ({});
        result.getHeapSpaceStatistics ||= () => [];
        result.getHeapCodeStatistics ||= () => ({});
        result.setFlagsFromString ||= () => undefined;
        result.cachedDataVersionTag ||= () => 0;
      }
      return result;
    };
  }
}
