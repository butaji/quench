const { randomBytes } = require("crypto");

for (const call of [() => randomBytes(-1), () => randomBytes(1, 123)]) {
  let error;
  try {
    call();
  } catch (caught) {
    error = caught;
  }
  if (!error) throw new Error("randomBytes accepted invalid arguments");
}

if (randomBytes(1.5).length !== 1) {
  throw new Error("randomBytes did not truncate fractional size");
}

console.log("crypto random bytes validation passed");
