const assert = require("assert");
const cluster = require("cluster");
const common = require("../../node/test/common");

if (cluster.isPrimary) {
  const events = [];
  const clusterEvents = {
    fork: false,
    online: false,
    listening: false,
    exit: false
  };
  const workerEvents = { online: false, listening: false, exit: false };
  for (const [name, index] of Object.keys(clusterEvents).map((name, index) => [
    name,
    index
  ])) {
    common.mustCallAtLeast((bool, eventName, eventIndex) => {
      cluster.on(
        eventName,
        common.mustCall(function (value, code, signal) {
          clusterEvents[eventName] = true;
          events.push(["cluster", eventName, value, code, signal]);
          assert.strictEqual(value, worker);
          if (eventName === "fork") assert.strictEqual(value.state, "none");
          if (eventName === "online") assert.strictEqual(value.state, "online");
          if (eventName === "listening")
            assert.strictEqual(value.state, "listening");
          if (eventName === "exit") assert.strictEqual(value.state, "dead");
        })
      );
    })(false, name, index);
  }
  cluster.on(
    "listening",
    common.mustCall(() => {})
  );
  cluster.on(
    "exit",
    common.mustCall(() => {})
  );
  const worker = cluster.fork();
  for (const [name] of Object.entries(workerEvents)) {
    worker.on(
      name,
      common.mustCall(function (code, signal) {
        workerEvents[name] = true;
        events.push(["worker", name, this, code, signal]);
        assert.strictEqual(this, worker);
        if (name === "online") assert.strictEqual(this.state, "online");
        if (name === "listening") {
          assert.strictEqual(this.state, "listening");
          worker.kill("SIGTERM");
        }
        if (name === "exit") assert.strictEqual(this.state, "dead");
      })
    );
  }
  worker.on(
    "exit",
    common.mustCall(function (code, signal) {
      events.push(["worker", "exit-check", this, code, signal]);
      assert.deepStrictEqual(
        events.map(([scope, name]) => `${scope}:${name}`),
        [
          "cluster:fork",
          "cluster:online",
          "worker:online",
          "cluster:listening",
          "worker:listening",
          "cluster:exit",
          "worker:exit",
          "worker:exit-check"
        ]
      );
      assert.strictEqual(worker.process.exitCode, code);
      assert.strictEqual(worker.process.signalCode, signal);
      process.exit(0);
    })
  );
} else {
  const http = require("http");
  new http.Server(common.mustNotCall()).listen(0, "127.0.0.1");
}
