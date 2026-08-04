for (let index = 0; index < 10; index += 1) {
  setTimeout(() => {}, index);
  setImmediate(() => {});
}

const resources = process.getActiveResourcesInfo();
const timeouts = resources.filter((type) => type === "Timeout");
const immediates = resources.filter((type) => type === "Immediate");
if (timeouts.length !== 10 || immediates.length !== 10) {
  throw new Error(`unexpected active timer resources: ${resources}`);
}
