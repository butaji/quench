const assert = require("assert");
const EventEmitter = require("events");
const http = require("http");

class FakeSocket extends EventEmitter {
  constructor(message = { shouldKeepAlive: true }) {
    super();
    this._httpMessage = message;
    this.destroyed = false;
    this.writable = true;
    this.timeout = 0;
    this.keepAlive = undefined;
    this.keepAliveDelay = undefined;
    this.unrefCalled = false;
    this.refCalled = false;
  }

  setKeepAlive(value, delay) {
    this.keepAlive = value;
    this.keepAliveDelay = delay;
  }

  setTimeout(value) {
    this.timeout = value;
  }

  unref() {
    this.unrefCalled = true;
  }

  ref() {
    this.refCalled = true;
  }

  destroy() {
    this.destroyed = true;
    this.writable = false;
  }
}

const options = { host: "agent.example", port: 8080 };
assert.strictEqual(
  new http.Agent({ agentKeepAliveTimeoutBuffer: 1500 })
    .agentKeepAliveTimeoutBuffer,
  1500,
);
assert.strictEqual(
  new http.Agent({ agentKeepAliveTimeoutBuffer: -100 })
    .agentKeepAliveTimeoutBuffer,
  1000,
);
assert.strictEqual(
  new http.Agent({ agentKeepAliveTimeoutBuffer: Infinity })
    .agentKeepAliveTimeoutBuffer,
  1000,
);
const agent = new http.Agent({
  keepAlive: true,
  keepAliveMsecs: 250,
  maxSockets: 2,
  maxFreeSockets: 1,
});
const name = agent.getName(options);
const socket = new FakeSocket();
agent.sockets[name] = [socket];
agent.totalSocketCount = 1;

agent.emit("free", socket, options);
assert.deepStrictEqual(agent.sockets[name], undefined);
assert.strictEqual(agent.freeSockets[name].length, 1);
assert.strictEqual(agent.freeSockets[name][0], socket);
assert.strictEqual(socket._httpMessage, null);
assert.strictEqual(socket.keepAlive, true);
assert.strictEqual(socket.keepAliveDelay, 250);
assert.strictEqual(socket.timeout, 0);
assert.strictEqual(socket.unrefCalled, true);

const request = {};
agent.reuseSocket(socket, request);
assert.strictEqual(request.reusedSocket, true);
assert.strictEqual(socket.refCalled, true);

const rejected = new FakeSocket();
agent.emit("free", rejected, options);
assert.strictEqual(rejected.destroyed, true);
assert.strictEqual(agent.freeSockets[name].length, 1);

const notKeepAlive = new FakeSocket({ shouldKeepAlive: false });
agent.emit("free", notKeepAlive, options);
assert.strictEqual(notKeepAlive.destroyed, true);

socket.writable = false;
agent.removeSocket(socket, options);
assert.strictEqual(agent.freeSockets[name], undefined);

console.log("http Agent free-socket bookkeeping passed");
