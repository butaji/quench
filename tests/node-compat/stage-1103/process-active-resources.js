setTimeout(() => {}, 0);
const resources = process.getActiveResourcesInfo();
if (resources.length !== 1 || resources[0] !== "Timeout") {
  throw new Error(`unexpected active resources: ${resources}`);
}
