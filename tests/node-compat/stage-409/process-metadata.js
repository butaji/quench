if (process.platform !== "darwin" && process.platform !== "linux") {
  throw new Error("process.platform has an unexpected value");
}
if (typeof process.arch !== "string" || typeof process.uptime() !== "number") {
  throw new Error("process metadata has the wrong types");
}

const memory = process.memoryUsage();
for (
  const name of ["rss", "heapTotal", "heapUsed", "external", "arrayBuffers"]
) {
  if (typeof memory[name] !== "number") {
    throw new Error("invalid memoryUsage shape");
  }
}

const resources = process.resourceUsage();
for (
  const name of [
    "userCPUTime",
    "systemCPUTime",
    "maxRSS",
    "fsRead",
    "fsWrite",
  ]
) {
  if (typeof resources[name] !== "number") {
    throw new Error("invalid resourceUsage shape");
  }
}

console.log("process metadata passed");
