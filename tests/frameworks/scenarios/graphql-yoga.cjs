const { createYoga, createSchema } = require('graphql-yoga'); const http = require('node:http'); const fs = require('node:fs'); const path = require('node:path');
const yoga = createYoga({ graphiql: false, maskedErrors: true, logging: false, schema: createSchema({
  typeDefs: 'type Query { hello(name: String): String, items: [Int] }',
  resolvers: { Query: { hello: (_, { name }) => `hi ${name}`, items: () => [1, 2, 3] } } }) });
// The shared request set over plain routes; GraphQL itself answers at /graphql.
const routes = { '/': (q, s) => { s.setHeader('content-type', 'text/html'); s.end('<p>hi</p>'); },
  '/echo': (q, s) => { let b = ''; q.on('data', d => b += d); q.on('end', () => { s.setHeader('content-type', 'application/json'); s.end(b); }); },
  '/stream': (q, s) => { s.setHeader('content-type', 'text/plain'); s.write('a'); s.write('b'); s.end('c'); },
  '/asset.txt': (q, s) => { s.setHeader('content-type', 'text/plain'); fs.createReadStream(path.join(__dirname, 'asset.txt')).pipe(s); } };
const query = (port, body) => new Promise((r, j) => { const q = http.request({ port, path: '/graphql', method: 'POST', headers: { 'content-type': 'application/json' } }, res => {
  let b = ''; res.on('data', d => b += d); res.on('end', () => r(`${res.statusCode} ${res.headers['content-type']} ${b}`)); }); q.on('error', j); q.end(JSON.stringify(body)); });
exports.start = () => new Promise(r => { const srv = http.createServer((q, s) => q.url.startsWith('/graphql') ? yoga(q, s) : (routes[q.url] || ((q, s) => { s.statusCode = 404; s.end('Not Found'); }))(q, s));
  srv.listen(0, async () => { const port = srv.address().port;
    console.log('graphql', await query(port, { query: '{ hello(name: "q") items }' }));
    console.log('graphql error', await query(port, { query: '{ nope }' }));
    r({ port, stop: () => new Promise(d => srv.close(d)) }); }); });
