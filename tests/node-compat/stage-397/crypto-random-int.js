const crypto = require("crypto");

for (let index = 0; index < 20; index++) {
  const value = crypto.randomInt(10, 20);
  if (value < 10 || value >= 20) throw new Error("randomInt range failed");
}

let error;
try {
  crypto.randomInt(5, 5);
} catch (caught) {
  error = caught;
}
if (!error || error.code !== "ERR_OUT_OF_RANGE") {
  throw new Error("randomInt must reject an empty range");
}

crypto.randomInt(2, 3, (callbackError, value) => {
  if (callbackError || value !== 2) {
    throw callbackError || new Error("randomInt callback failed");
  }
  console.log("crypto random int passed");
});
