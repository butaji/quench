const timeout = setTimeout(() => {
  const resources = process.getActiveResourcesInfo();
  if (resources.filter((type) => type === "Timeout").length !== 1) {
    throw new Error(`timeout was not active during callback: ${resources}`);
  }
  clearTimeout(timeout);
  if (process.getActiveResourcesInfo().includes("Timeout")) {
    throw new Error("cleared timeout remained active");
  }
}, 0);

if (!process.getActiveResourcesInfo().includes("Timeout")) {
  throw new Error("scheduled timeout was not active");
}

const interval = setInterval(() => {}, 1000);
if (
  process.getActiveResourcesInfo().filter((type) => type === "Timeout")
    .length !== 2
) {
  throw new Error("scheduled interval was not reported as a timeout");
}
clearInterval(interval);
