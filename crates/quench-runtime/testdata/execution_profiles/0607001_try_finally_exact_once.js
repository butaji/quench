function run(shouldThrow) {
  var effects = 0;
  try {
    effects++;
    if (shouldThrow) throw new Error("boom");
    effects += 10;
  } catch (error) {
    effects += error.message.length;
  } finally {
    effects += 100;
  }
  return effects;
}

run(false);
function profileRun() {
  return run(true);
}
function profileVerify(result) {
if (result !== 105) {
  throw new Error("try/finally exactly-once mismatch: " + result);
}
  return result;
}
return { run: profileRun, verify: profileVerify };
