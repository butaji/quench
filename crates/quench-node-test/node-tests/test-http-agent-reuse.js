// http — agent pools: a keep-alive Agent reuses one connection for sequential
// requests, honors maxSockets per origin by queueing, reconnects after a peer
// closes a free socket or answers Connection: close, expires free sockets with
// the agent `timeout` option, reports ECONNRESET for a request that ran on a
// stale free socket, and closes its sockets on destroy. The server is a raw
// `net` server so the test observes TCP connections directly; Quench's HTTP
// server does not expose per-connection counters.
'use strict';
const http = require('node:http');
const net = require('node:net');
const assert = require('assert');

const WATCHDOG_MS = 30000;
const watchdog = setTimeout(() => {
  console.error('http-agent-reuse: watchdog timeout');
  process.exit(1);
}, WATCHDOG_MS);
watchdog.unref?.();

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/// Write a response with a fixed body and optional extra header lines.
function respond(socket, { version = '1.1', headers = [] } = {}) {
  const extra = headers.map((name) => `${name}\r\n`).join('');
  socket.write(`HTTP/${version} 200 OK\r\nContent-Length: 2\r\n${extra}\r\nok`);
}

// A raw HTTP server: parses complete requests (bodies included), records them,
// and answers through `onRequest(request, socket, host)`.
function startServer(onRequest) {
  const sockets = [];
  const requests = [];
  const closed = [];
  const server = net.createServer((socket) => {
    const connection = sockets.push(socket) - 1;
    socket.on('error', () => {});
    socket.on('close', () => closed.push(connection));
    let buffer = '';
    socket.on('data', (chunk) => {
      buffer += chunk.toString('utf8');
      for (;;) {
        const headEnd = buffer.indexOf('\r\n\r\n');
        if (headEnd === -1) return;
        const lines = buffer.slice(0, headEnd).split('\r\n');
        const [method, path] = lines[0].split(' ');
        const headers = {};
        for (const line of lines.slice(1)) {
          const colon = line.indexOf(':');
          if (colon === -1) continue;
          const name = line.slice(0, colon).trim().toLowerCase();
          headers[name] = line.slice(colon + 1).trim();
        }
        const length = Number(headers['content-length'] ?? 0);
        if (buffer.length < headEnd + 4 + length) return;
        const body = buffer.slice(headEnd + 4, headEnd + 4 + length);
        buffer = buffer.slice(headEnd + 4 + length);
        const request = { method, path, headers, body, connection };
        requests.push(request);
        onRequest(request, socket, { requests, sockets, closed });
      }
    });
  });
  return {
    server,
    sockets,
    requests,
    closed,
    connections: () => sockets.length,
    close() {
      for (const socket of sockets) socket.destroy();
      server.close();
    },
  };
}

function listen(server) {
  return new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
}

/// Send one request; errors resolve as `{ error: code, reused }` so scenarios can
/// assert on either outcome.
function request(port, agent, options) {
  return new Promise((resolve) => {
    const outgoing = http.request(
      { host: '127.0.0.1', port, agent, path: '/', ...options },
      (response) => {
        let body = '';
        response.setEncoding('utf8');
        response.on('data', (chunk) => { body += chunk; });
        response.on('end', () => {
          resolve({
            status: response.statusCode,
            body,
            reused: outgoing.reusedSocket,
            hasReusedSocket: 'reusedSocket' in outgoing,
          });
        });
      },
    );
    outgoing.on('error', (error) => resolve({
      error: error.code,
      reused: outgoing.reusedSocket,
      hasReusedSocket: 'reusedSocket' in outgoing,
    }));
    outgoing.end(options.body);
  });
}

function get(port, agent, path) {
  return request(port, agent, { path, method: 'GET' });
}

async function reuseScenario() {
  const host = startServer((request, socket) => respond(socket));
  await listen(host.server);
  const port = host.server.address().port;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    for (let index = 0; index < 3; index += 1) {
      const response = await get(port, agent, `/reuse/${index}`);
      assert.strictEqual(response.status, 200);
      assert.strictEqual(response.body, 'ok');
      // Node's ClientRequest always carries `reusedSocket`, false until an agent
      // places the request on a free socket.
      assert.strictEqual(response.hasReusedSocket, true,
        `request ${index} has no reusedSocket property`);
      assert.strictEqual(response.reused, index > 0,
        `request ${index} reported reusedSocket ${response.reused}`);
    }
    assert.strictEqual(host.connections(), 1,
      `sequential keep-alive requests used ${host.connections()} connections`);
    // destroy() drops the free socket; the agent's next request reconnects.
    const socket = host.sockets[0];
    const closed = new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error('agent.destroy did not close its free socket')),
        2000,
      );
      socket.once('close', () => {
        clearTimeout(timer);
        resolve();
      });
    });
    agent.destroy();
    await closed;
    const afterDestroy = await get(port, agent, '/after-destroy');
    assert.strictEqual(afterDestroy.status, 200);
    assert.strictEqual(host.connections(), 2,
      'a destroyed agent must not reuse its free socket');
    agent.destroy();
  } finally {
    host.close();
  }
}

async function maxSocketsScenario() {
  const host = startServer((request, socket) => {
    setTimeout(() => respond(socket), 15);
  });
  await listen(host.server);
  const port = host.server.address().port;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    const responses = await Promise.all([0, 1, 2].map((index) =>
      get(port, agent, `/queued/${index}`)));
    for (const response of responses) {
      assert.strictEqual(response.status, 200);
      assert.strictEqual(response.body, 'ok');
      // Node hands a freed socket to a queued request through the agent's `free`
      // handler, which leaves `reusedSocket` false.
      assert.strictEqual(response.reused, false,
        'a queued request that continued on a freed socket must not report reuse');
    }
    assert.strictEqual(host.connections(), 1,
      `maxSockets:1 opened ${host.connections()} connections`);
    agent.destroy();
  } finally {
    host.close();
  }
}

async function peerClosedFreeSocketScenario() {
  const host = startServer((request, socket) => {
    respond(socket);
    if (request.path === '/first') {
      // End the idle connection without a Connection: close header.
      setTimeout(() => socket.end(), 5);
    }
  });
  await listen(host.server);
  const port = host.server.address().port;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    const first = await get(port, agent, '/first');
    assert.strictEqual(first.status, 200);
    await sleep(80);
    const second = await get(port, agent, '/second');
    assert.strictEqual(second.status, 200, `second request failed: ${second.error}`);
    assert.strictEqual(second.body, 'ok');
    assert.strictEqual(second.reused ?? false, false,
      'a free socket the peer closed must not be reused');
    assert.strictEqual(host.connections(), 2,
      `a peer-closed free socket left ${host.connections()} connections`);
    assert.deepStrictEqual(host.requests.map((entry) => entry.path), ['/first', '/second']);
    agent.destroy();
  } finally {
    host.close();
  }
}

async function strayBytesOnFreeSocketScenario() {
  // Bytes that arrive on an idle connection are never a response. Node destroys
  // the poisoned free socket instead of parsing them as the next response.
  const host = startServer((request, socket) => {
    respond(socket);
    if (request.path === '/first') {
      setTimeout(() => socket.write('HTTP/1.1 200 OK\r\n'), 20);
    }
  });
  await listen(host.server);
  const port = host.server.address().port;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    const first = await get(port, agent, '/first');
    assert.strictEqual(first.status, 200);
    await sleep(120);
    const second = await get(port, agent, '/second');
    assert.strictEqual(second.status, 200, `second request failed: ${second.error}`);
    assert.strictEqual(second.body, 'ok', `second response body was ${JSON.stringify(second.body)}`);
    assert.strictEqual(second.reused, false, 'a poisoned free socket must not be reused');
    assert.strictEqual(host.connections(), 2,
      `stray bytes on a free socket left ${host.connections()} connections`);
    agent.destroy();
  } finally {
    host.close();
  }
}

async function staleReusedSocketScenario() {
  // The peer destroys the connection that carried the second (reused) request
  // instead of answering it. The request must fail with ECONNRESET exactly once:
  // Node never replays a request whose bytes already left the client.
  const host = startServer((request, socket) => {
    if (request.path === '/first') {
      respond(socket);
      return;
    }
    socket.destroy();
  });
  await listen(host.server);
  const port = host.server.address().port;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    const first = await request(port, agent, {
      path: '/first', method: 'POST', body: 'alpha',
      headers: { 'content-length': '5' },
    });
    assert.strictEqual(first.status, 200);
    assert.strictEqual(first.reused, false);
    const second = await request(port, agent, {
      path: '/second', method: 'POST', body: 'beta',
      headers: { 'content-length': '4' },
    });
    assert.strictEqual(second.error, 'ECONNRESET',
      `stale reused socket failed with ${second.error} instead of ECONNRESET`);
    assert.strictEqual(second.reused, true, 'a stale reused socket must report reuse');
    assert.deepStrictEqual(
      host.requests.map((entry) => [entry.method, entry.path, entry.body]),
      [['POST', '/first', 'alpha'], ['POST', '/second', 'beta']],
      'the replayed request must not reach the server twice',
    );
    assert.strictEqual(host.connections(), 1,
      `a reset reused socket left ${host.connections()} connections`);
    agent.destroy();
  } finally {
    host.close();
  }
}

async function originCapacityScenario() {
  // maxSockets limits one origin, not the agent: a slow origin must not make a
  // request to another origin wait for its socket.
  const slow = startServer((request, socket) => {
    setTimeout(() => respond(socket), 250);
  });
  const fast = startServer((request, socket) => respond(socket));
  await listen(slow.server);
  await listen(fast.server);
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  const order = [];
  try {
    const record = (label) => (response) => {
      order.push(label);
      return response;
    };
    const responses = await Promise.all([
      get(slow.server.address().port, agent, '/slow').then(record('slow')),
      get(fast.server.address().port, agent, '/fast').then(record('fast')),
    ]);
    for (const response of responses) {
      assert.strictEqual(response.status, 200, `request failed: ${response.error}`);
    }
    assert.deepStrictEqual(order, ['fast', 'slow'],
      `maxSockets:1 serialized two origins in the order ${order.join(',')}`);
    assert.strictEqual(slow.connections(), 1);
    assert.strictEqual(fast.connections(), 1);
    agent.destroy();
  } finally {
    slow.close();
    fast.close();
  }
}

async function freeSocketLifetimeScenario() {
  // keepAliveMsecs is the TCP keep-alive probe delay; the free-socket lifetime
  // comes from the agent `timeout` option, where 0 never expires.
  const cases = [
    { label: 'keepAliveMsecs', options: { keepAliveMsecs: 60 }, gap: 200, connections: 1 },
    { label: 'timeout', options: { timeout: 80 }, gap: 300, connections: 2 },
    { label: 'timeout0', options: { timeout: 0 }, gap: 300, connections: 1 },
  ];
  for (const { label, options, gap, connections } of cases) {
    const host = startServer((request, socket) => respond(socket));
    await listen(host.server);
    const port = host.server.address().port;
    const agent = new http.Agent({ keepAlive: true, maxSockets: 1, ...options });
    try {
      const first = await get(port, agent, '/first');
      assert.strictEqual(first.status, 200);
      await sleep(gap);
      const second = await get(port, agent, '/second');
      assert.strictEqual(second.status, 200, `${label}: second request failed: ${second.error}`);
      assert.strictEqual(host.connections(), connections,
        `${label}: ${gap}ms later the agent had ${host.connections()} connections`);
      assert.strictEqual(second.reused, connections === 1,
        `${label}: reusedSocket was ${second.reused}`);
    } finally {
      agent.destroy();
      host.close();
    }
  }
}

async function destroyWithQueuedRequestScenario() {
  const host = startServer((request, socket) => {
    setTimeout(() => respond(socket), 200);
  });
  await listen(host.server);
  const port = host.server.address().port;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    const inFlight = get(port, agent, '/in-flight');
    const queued = get(port, agent, '/queued');
    await sleep(50);
    // Node destroys the agent's sockets; the request it queued continues on a
    // fresh connection instead of being aborted.
    agent.destroy();
    const [first, second] = await Promise.all([inFlight, queued]);
    assert.strictEqual(first.error, 'ECONNRESET',
      `a destroyed agent failed its socket with ${first.error} instead of ECONNRESET`);
    assert.strictEqual(second.status, 200,
      `a queued request did not survive agent.destroy: ${second.error}`);
    assert.strictEqual(second.body, 'ok');
    const later = await get(port, agent, '/later');
    assert.strictEqual(later.status, 200, `the destroyed agent stopped working: ${later.error}`);
    assert.strictEqual(host.connections(), 2,
      `agent.destroy left ${host.connections()} connections`);
    agent.destroy();
  } finally {
    host.close();
  }
}

async function httpVersionScenario() {
  // HTTP/1.0 connections persist only on an explicit keep-alive, and an HTTP/1.1
  // response that closes the connection ends its socket.
  const cases = [
    { label: 'http/1.0', version: '1.0', responseHeaders: [], connections: 2, reused: false },
    {
      label: 'http/1.0 keep-alive', version: '1.0',
      responseHeaders: ['Connection: keep-alive'], connections: 1, reused: true,
    },
    {
      label: 'http/1.1 connection: close', version: '1.1',
      responseHeaders: ['Connection: close'], connections: 2, reused: false,
    },
  ];
  for (const { label, version, responseHeaders, connections, reused } of cases) {
    const host = startServer((request, socket) => respond(socket, { version, headers: responseHeaders }));
    await listen(host.server);
    const port = host.server.address().port;
    const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
    try {
      const first = await get(port, agent, '/first');
      assert.strictEqual(first.status, 200);
      const second = await get(port, agent, '/second');
      assert.strictEqual(second.status, 200, `${label}: second request failed: ${second.error}`);
      assert.strictEqual(second.body, 'ok');
      assert.strictEqual(host.connections(), connections,
        `${label}: the agent opened ${host.connections()} connections`);
      assert.strictEqual(second.reused, reused, `${label}: reusedSocket was ${second.reused}`);
    } finally {
      agent.destroy();
      host.close();
    }
  }
}

async function requestConnectionCloseScenario() {
  // A server that honors a request's Connection: close ends the connection, so
  // the next request needs a new one.
  const host = startServer((request, socket) => {
    respond(socket, {
      headers: (request.headers.connection ?? '').includes('close') ? ['Connection: close'] : [],
    });
    if ((request.headers.connection ?? '').includes('close')) socket.end();
  });
  await listen(host.server);
  const port = host.server.address().port;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    for (let index = 0; index < 2; index += 1) {
      const response = await request(port, agent, {
        path: `/close/${index}`,
        method: 'GET',
        headers: { connection: 'close' },
      });
      assert.strictEqual(response.status, 200, `request failed: ${response.error}`);
      assert.strictEqual(response.body, 'ok');
      assert.strictEqual(response.reused, false, 'Connection: close must not reuse a socket');
    }
    assert.deepStrictEqual(host.requests.map((entry) => entry.headers.connection), ['close', 'close']);
    assert.strictEqual(host.connections(), 2,
      `Connection: close left ${host.connections()} connections`);
    agent.destroy();
  } finally {
    host.close();
  }
}

async function main() {
  await reuseScenario();
  await maxSocketsScenario();
  await peerClosedFreeSocketScenario();
  await strayBytesOnFreeSocketScenario();
  await staleReusedSocketScenario();
  await originCapacityScenario();
  await freeSocketLifetimeScenario();
  await destroyWithQueuedRequestScenario();
  await httpVersionScenario();
  await requestConnectionCloseScenario();
  clearTimeout(watchdog);
  console.log('http-agent-reuse: ok');
}

main().catch((error) => {
  clearTimeout(watchdog);
  console.error(error && error.stack || error);
  process.exit(1);
});
