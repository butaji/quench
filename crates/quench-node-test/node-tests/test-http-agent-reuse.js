// http — agent pools: a keep-alive Agent reuses one connection for sequential
// requests, honors maxSockets by queueing, reconnects after Connection: close,
// and closes its free socket on destroy. The server is a raw `net` server so
// the test observes TCP connections directly; Quench's HTTP server does not
// expose per-connection counters.
'use strict';
const http = require('node:http');
const net = require('node:net');
const assert = require('assert');

const WATCHDOG_MS = 15000;
const watchdog = setTimeout(() => {
  console.error('http-agent-reuse: watchdog timeout');
  process.exit(1);
}, WATCHDOG_MS);
watchdog.unref?.();

function startServer({ delay = 0, close = false } = {}) {
  const sockets = [];
  let active = 0;
  let peak = 0;
  const server = net.createServer((socket) => {
    sockets.push(socket);
    socket.on('error', () => {});
    let buffer = '';
    const respond = () => {
      active += 1;
      peak = Math.max(peak, active);
      const head = close
        ? 'HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n'
        : 'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n';
      const finish = () => {
        active -= 1;
        socket.write(`${head}ok`);
        if (close) socket.end();
      };
      if (delay > 0) setTimeout(finish, delay);
      else finish();
    };
    socket.on('data', (chunk) => {
      buffer += chunk.toString('utf8');
      let end = buffer.indexOf('\r\n\r\n');
      while (end !== -1) {
        buffer = buffer.slice(end + 4);
        respond();
        end = buffer.indexOf('\r\n\r\n');
      }
    });
  });
  return {
    server,
    sockets,
    connections: () => sockets.length,
    peak: () => peak,
    close: () => server.close(),
  };
}

function listen(server) {
  return new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
}

function get(port, agent, path) {
  return new Promise((resolve, reject) => {
    const request = http.get({ host: '127.0.0.1', port, path, agent }, (response) => {
      let body = '';
      response.setEncoding('utf8');
      response.on('data', (chunk) => { body += chunk; });
      response.on('end', () => resolve({ status: response.statusCode, body }));
    });
    request.on('error', reject);
  });
}

async function reuseScenario() {
  const host = startServer();
  await listen(host.server);
  const port = host.server.address().port;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    for (let index = 0; index < 3; index += 1) {
      const response = await get(port, agent, `/reuse/${index}`);
      assert.strictEqual(response.status, 200);
      assert.strictEqual(response.body, 'ok');
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
  const host = startServer({ delay: 15 });
  await listen(host.server);
  const port = host.server.address().port;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    const responses = await Promise.all([0, 1, 2].map((index) =>
      get(port, agent, `/queued/${index}`)));
    for (const response of responses) {
      assert.strictEqual(response.status, 200);
      assert.strictEqual(response.body, 'ok');
    }
    assert.strictEqual(host.peak(), 1,
      `maxSockets:1 served ${host.peak()} requests at once`);
    assert.strictEqual(host.connections(), 1,
      `maxSockets:1 opened ${host.connections()} connections`);
    agent.destroy();
  } finally {
    host.close();
  }
}

async function connectionCloseScenario() {
  const host = startServer({ close: true });
  await listen(host.server);
  const port = host.server.address().port;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  try {
    for (let index = 0; index < 2; index += 1) {
      const response = await get(port, agent, `/close/${index}`);
      assert.strictEqual(response.status, 200);
      assert.strictEqual(response.body, 'ok');
    }
    assert.strictEqual(host.connections(), 2,
      'Connection: close sockets must not be reused');
    agent.destroy();
  } finally {
    host.close();
  }
}

async function main() {
  await reuseScenario();
  await maxSocketsScenario();
  await connectionCloseScenario();
  clearTimeout(watchdog);
  console.log('http-agent-reuse: ok');
}

main().catch((error) => {
  clearTimeout(watchdog);
  console.error(error && error.stack || error);
  process.exit(1);
});
