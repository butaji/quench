// Lightweight Fastify-compatible factory for node:fastify.
(function (deps) {
  'use strict';
  function fastify(opts) {
    var routes = [];
    var instance = {
      get: function (url, handler) { routes.push({ method: 'GET', url: url, handler: handler }); return instance; },
      post: function (url, handler) { routes.push({ method: 'POST', url: url, handler: handler }); return instance; },
      route: function (spec) { routes.push(spec); return instance; },
      ready: function () { return Promise.resolve(instance); },
      close: function () { return Promise.resolve(); },
      listen: function (options, cb) {
        var port = typeof options === 'number' ? options : (options && options.port) || 0;
        var server = require('node:http').createServer(function (req, res) {
          var found, i;
          for (i = 0; i < routes.length; i++) if (routes[i].method === req.method && routes[i].url === req.url) { found = routes[i]; break; }
          if (!found) { res.statusCode = 404; return res.end('Route GET not found'); }
          var reply = { code: function (n) { res.statusCode = n; return reply; }, send: function (body) { res.end(typeof body === 'object' ? JSON.stringify(body) : String(body)); } };
          Promise.resolve(found.handler({ method: req.method, url: req.url, raw: req }, reply)).then(function (body) { if (body !== undefined && !res.writableEnded) reply.send(body); }).catch(function (err) { res.statusCode = 500; res.end(String(err)); });
        });
        var done = function (err) { if (cb) cb(err, server); };
        return server.listen(port, done);
      },
      inject: function (request) {
        var i, found;
        for (i = 0; i < routes.length; i++) if (routes[i].method === request.method && routes[i].url === request.url) { found = routes[i]; break; }
        if (!found) return Promise.resolve({ statusCode: 404, payload: 'Route GET not found' });
        return Promise.resolve(found.handler(request, { code: function () { return this; }, send: function (x) { return x; } })).then(function (x) { return { statusCode: 200, payload: x === undefined ? '' : String(x) }; });
      }
    };
    return instance;
  }
  return fastify;
});
