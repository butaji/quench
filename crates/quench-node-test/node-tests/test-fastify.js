const assert = require('node:assert');
const fastify = require('node:fastify')();
fastify.get('/hello', async () => 'world');
assert.strictEqual(typeof fastify.listen, 'function');
fastify.inject({ method: 'GET', url: '/hello' }).then(result => {
  assert.strictEqual(result.statusCode, 200);
  assert.strictEqual(result.payload, 'world');
  return fastify.ready();
}).then(() => console.log('fastify compatibility ok'));
