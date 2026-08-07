const __quenchClusterApiRequire = globalThis.require;
const __quenchClusterApi = __quenchClusterApiRequire("cluster");
if (typeof __quenchClusterApi.Worker?.prototype.isConnected !== "function") {
  __quenchClusterApi.Worker.prototype.isConnected = function () {
    return this.state !== "dead" && this.state !== "disconnected";
  };
}
