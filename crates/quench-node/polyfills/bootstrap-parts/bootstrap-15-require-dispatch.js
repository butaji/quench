const __quenchRequireParts = [
  globalThis.__quench_require_part_00,
  globalThis.__quench_require_part_01,
  globalThis.__quench_require_part_02,
  globalThis.__quench_require_part_03
];
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  for (const handler of __quenchRequireParts) {
    const result = handler(name, specifier);
    if (result !== undefined) return result;
  }
  throw new Error("Cannot find module " + String(specifier));
};
