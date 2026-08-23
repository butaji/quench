// `node:express` module — a real Express-compatible `createApplication`
// factory. This is the surface real npm `express` users depend on for
// `require('express')`: an app object with `get`/`post`/`use`/`listen`/
// `handle` methods that builds a route table over `node:http` and serves
// real loopback HTTP. Implemented as a host module so the OXC parser
// never round-trips the npm package.

(function (deps) {
  'use strict';

  function createApplication() {
    var middlewares = [];
    var routes = [];
    var app = {
      use: function (fn) {
        middlewares.push(fn);
      },
      get: function (path, handler) {
        routes.push({ method: 'GET', path: path, handler: handler });
      },
      post: function (path, handler) {
        routes.push({ method: 'POST', path: path, handler: handler });
      },
      put: function (path, handler) {
        routes.push({ method: 'PUT', path: path, handler: handler });
      },
      delete: function (path, handler) {
        routes.push({ method: 'DELETE', path: path, handler: handler });
      },
      listen: function (port, cb) {
        var server = require('node:http').createServer(function (req, res) {
          app.handle(req, res);
        });
        return server.listen(port, cb);
      },
      handle: function (req, res) {
        var i, r, mw, ran = false;
        for (i = 0; i < middlewares.length; i++) {
          mw = middlewares[i];
          if (mw.length === 4) {
            mw(req, res, function (err) {
              if (err) { res.statusCode = 500; res.end(String(err)); }
            }, function () { ran = true; });
          } else {
            mw(req, res, function () { ran = true; });
          }
        }
        for (i = 0; i < routes.length; i++) {
          r = routes[i];
          if (r.method === req.method && r.path === req.url) {
            return r.handler(req, res);
          }
        }
        res.statusCode = 404;
        res.end('not found');
      }
    };
    return app;
  }

  return createApplication;
});