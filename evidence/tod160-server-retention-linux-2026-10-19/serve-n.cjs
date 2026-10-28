// quench-node serve-n.cjs SCENARIO LIMIT : serve until LIMIT requests completed, then stop
// the server so the process ends normally (and memory profiles report "complete").
const http = require('node:http');
const limit = +process.argv[3]; let seen = 0, stopServer;
const emit = http.Server.prototype.emit;
http.Server.prototype.emit = function (event, request, response) {
  if (event === 'request') response.on('finish', () => { if (++seen === limit) setImmediate(() => stopServer()); });
  return emit.apply(this, arguments);
};
const app = require(require('node:path').resolve(process.argv[2]));
app.start().then(({ port, stop }) => { stopServer = () => { if (typeof gc === 'function') gc(); return stop(); }; console.log('PORT ' + port); });
