'use strict';
const assert = require('assert');
const http2 = require('http2');
const server = http2.createServer();
const expected = new Error('session error control');
let session;
let sessionListenerSawError = false;
let serverListenerSawError = false;
let client;
server.on('sessionError', (error, receivedSession) => {
  assert.strictEqual(error, expected);
  assert.strictEqual(receivedSession, session);
  serverListenerSawError = true;
  if (client) client.destroy();
  server.close(() => {
    assert.strictEqual(sessionListenerSawError, true);
    assert.strictEqual(serverListenerSawError, true);
    console.log('session error and server sessionError listeners both received the same error');
  });
});
server.on('session', (receivedSession) => {
  session = receivedSession;
  session.on('error', (error) => {
    assert.strictEqual(error, expected);
    sessionListenerSawError = true;
  });
  session.destroy(expected);
});
server.listen(0, () => {
  client = http2.connect(`http://localhost:${server.address().port}`);
  client.on('error', () => {});
  client.request().resume();
});
setTimeout(() => { throw new Error('control timed out'); }, 5000).unref();
