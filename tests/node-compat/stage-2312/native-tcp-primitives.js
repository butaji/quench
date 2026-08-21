const assert = require("assert");

const listener = __quench_tcp_bind("127.0.0.1", 0);
const port = __quench_tcp_bound_port(listener);
assert.ok(port > 0);

const client = __quench_tcp_connect("127.0.0.1", port);
let server = 0;
for (let attempt = 0; attempt < 100 && !server; attempt++) {
  server = __quench_tcp_accept(listener);
  if (!server) __quench_sleep_ms(1);
}
assert.ok(server);

assert.strictEqual(__quench_tcp_write(client, [1, 2, 3]), 3);
let received = [];
for (let attempt = 0; attempt < 100 && received.length === 0; attempt++) {
  received = __quench_tcp_read(server);
  if (received.length === 0) __quench_sleep_ms(1);
}
assert.deepStrictEqual(received, [1, 2, 3]);

__quench_tcp_close(client);
__quench_tcp_close(server);
__quench_tcp_close(listener);
console.log("native TCP primitives passed");
