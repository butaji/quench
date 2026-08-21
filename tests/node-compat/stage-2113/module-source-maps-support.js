const assert = require("assert");
const moduleApi = require("module");

for (const value of [undefined, null, 1, {}, () => {}]) {
  assert.throws(
    () => moduleApi.setSourceMapsSupport(value),
    (error) => error.code === "ERR_INVALID_ARG_TYPE",
  );
}
for (const value of [null, 1, {}, () => {}]) {
  assert.throws(
    () => moduleApi.setSourceMapsSupport(true, { nodeModules: value }),
    (error) => error.code === "ERR_INVALID_ARG_TYPE",
  );
  assert.throws(
    () => moduleApi.setSourceMapsSupport(true, { generatedCode: value }),
    (error) => error.code === "ERR_INVALID_ARG_TYPE",
  );
}
moduleApi.setSourceMapsSupport(true, {
  nodeModules: true,
  generatedCode: false,
});

console.log("module source maps support pass");
