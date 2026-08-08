const assert = require("assert");

const listener = __quench_tcp_bind("127.0.0.1", 0);
const port = __quench_tcp_bound_port(listener);
const client = __quench_tcp_connect("127.0.0.1", port);
let server = 0;
for (let attempt = 0; attempt < 100 && !server; attempt++) {
  server = __quench_tcp_accept(listener);
  if (!server) __quench_sleep_ms(1);
}
assert.ok(server);
assert.strictEqual(__quench_tcp_readable(server), 0);
assert.strictEqual(__quench_tcp_write(client, [42]), 1);
for (let attempt = 0; attempt < 100; attempt++) {
  if (__quench_tcp_readable(server)) break;
  __quench_sleep_ms(1);
}
assert.strictEqual(__quench_tcp_readable(server), 1);
assert.deepStrictEqual(__quench_tcp_read(server), [42]);
__quench_tcp_close(client);
__quench_tcp_close(server);
__quench_tcp_close(listener);
console.log("native TCP readiness passed");
