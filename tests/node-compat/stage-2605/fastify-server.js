const Fastify = require('fastify');
const app = Fastify({ logger: false });
app.get('/health', async () => ({ ok: true, framework: 'fastify' }));
app.listen({ port: 3457, host: '127.0.0.1' });
