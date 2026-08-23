(function () {
  'use strict';
  function Hono() {
    var routes = [];
    var app = {
      get: function (path, handler) { routes.push({ method: 'GET', path: path, handler: handler }); return app; },
      post: function (path, handler) { routes.push({ method: 'POST', path: path, handler: handler }); return app; },
      put: function (path, handler) { routes.push({ method: 'PUT', path: path, handler: handler }); return app; },
      delete: function (path, handler) { routes.push({ method: 'DELETE', path: path, handler: handler }); return app; },
      on: function (method, path, handler) { routes.push({ method: method, path: path, handler: handler }); return app; },
      fetch: function (request) {
        var method = request.method || 'GET';
        var path = new URL(request.url || 'http://localhost/').pathname;
        var route = routes.find(function (item) { return item.method === method && item.path === path; });
        var context = {
          req: request,
          request: request,
          env: {},
          json: function (value, init) { return new Response(JSON.stringify(value), { status: init && init.status || 200, headers: { 'content-type': 'application/json' } }); },
          text: function (value, init) { return new Response(String(value), { status: init && init.status || 200 }); },
          body: function (value, init) { return new Response(value, init); },
          status: function () {}
        };
        if (!route) return Promise.resolve(new Response('Not Found', { status: 404 }));
        return Promise.resolve(route.handler(context));
      },
      request: function (input, init) { return app.fetch(new Request(input, init)); },
      fire: function (request) { return app.fetch(request); }
    };
    return app;
  }
  return Hono;
});
