// Layout training workload (tools/bolt-layout.sh): the npm package under QUENCH_LAYOUT_NODE_MODULES.
const ptr = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/path-to-regexp'); const match = ptr.match;
const routes = ['/users/:id', '/users/:id/posts/:post', '/files/*path', '/api/v1/:resource/:action', '/blog/:year/:month/:slug', '/static/{*rest}'].filter(r => { try { match(r); return true; } catch { return false; } });
const ms = routes.map(r => match(r, { decode: decodeURIComponent }));
let n = 0; for (let i = 0; i < 3000; i++) { const u = ['/users/' + i, '/users/' + i + '/posts/' + (i * 7), '/api/v1/item' + (i % 9) + '/get', '/blog/2024/' + (i % 12) + '/post-' + i, '/files/a/b/' + i + '.txt'][i % 5]; for (const m of ms) if (m(u)) n++; }
console.log(n, routes.length);
