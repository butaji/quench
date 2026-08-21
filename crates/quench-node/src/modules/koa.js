// Lightweight Koa-compatible application factory for node:koa.
(function (deps) {
  'use strict';
  function Koa() {
    var middleware = [];
    var app = {
      middleware: middleware,
      use: function (fn) { middleware.push(fn); return app; },
      callback: function () {
        return function (req, res) {
          var ctx = { req: req, request: { req: req }, response: { res: res }, res: res,
            state: {}, status: 200, body: undefined, method: req.method, url: req.url };
          var index = -1;
          function run(i) {
            if (i <= index) { return Promise.reject(new Error('next() called multiple times')); }
            index = i;
            var fn = middleware[i];
            if (!fn) {
              res.statusCode = ctx.status || 200;
              if (ctx.body !== undefined) {
                if (typeof ctx.body === 'object') {
                  res.setHeader('content-type', 'application/json');
                  res.end(JSON.stringify(ctx.body));
                } else { res.end(String(ctx.body)); }
              } else { res.end(); }
              return Promise.resolve();
            }
            return Promise.resolve(fn(ctx, function () { return run(i + 1); }));
          }
          run(0).catch(function (err) { res.statusCode = 500; res.end(String(err)); });
        };
      },
      listen: function (port, cb) {
        var server = require('node:http').createServer(app.callback());
        return server.listen(port, cb);
      }
    };
    return app;
  }
  return Koa;
});
