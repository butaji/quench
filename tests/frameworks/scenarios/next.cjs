const path = require('path'); const http = require('http');

exports.start = async () => { const next = require('next'); const a = next({ dev: false, dir: path.join(__dirname, '..', 'next-app') }); await a.prepare(); const h = a.getRequestHandler();
  return new Promise(r => { const srv = http.createServer((q, s) => h(q, s)); srv.listen(0, () => r({ port: srv.address().port, stop: () => new Promise(d => { srv.closeAllConnections(); srv.close(d); }) })); }); };
