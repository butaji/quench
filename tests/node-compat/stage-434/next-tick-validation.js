let error;
try {
  process.nextTick(null);
} catch (caught) {
  error = caught;
}
if (!error || error.code !== "ERR_INVALID_ARG_TYPE") {
  throw new Error("nextTick must validate its callback synchronously");
}

console.log("next tick validation passed");
