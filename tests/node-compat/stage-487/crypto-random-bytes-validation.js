const { randomBytes } = require("crypto");

for (
  const call of [
    () => randomBytes(-1),
    () => randomBytes(1.5),
    () => randomBytes(1, 123),
  ]
) {
  let error;
  try {
    call();
  } catch (caught) {
    error = caught;
  }
  if (!error) throw new Error("randomBytes accepted invalid arguments");
}

console.log("crypto random bytes validation passed");
