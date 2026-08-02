const fs = require("fs");

let error;
try {
  fs.closeSync(999999);
} catch (caught) {
  error = caught;
}

if (!error || error.code !== "EBADF" || error.syscall !== "close") {
  throw new Error("closeSync must reject unknown descriptors with EBADF");
}

console.log("fs close invalid fd passed");
