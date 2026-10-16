'use strict';
const assert = require('assert');
const net = require('net');
const { TCP, constants } = require('internal/test/binding').internalBinding('tcp_wrap');
const server = new TCP(constants.SERVER);
assert.equal(server.bind('127.0.0.1', 0), 0);
const address = {};
assert.equal(server.getsockname(address), 0);
let accepts = 0, closes = 0, socketClosed = false, reported = false;
function finish() {
  if (closes === 2 && socketClosed && !reported) {
    reported = true;
    console.log('actual native accept/prototype/callback identity pass');
  }
}
server.onconnection = function(status, client) {
  assert.equal(this, server);
  assert.equal(status, 0);
  assert.strictEqual(Object.getPrototypeOf(client), TCP.prototype);
  assert.equal(client.writeQueueSize, 0);
  accepts++;
  client.close(function() { assert.equal(this, client); closes++; finish(); });
  server.close(function() { assert.equal(this, server); closes++; finish(); });
};
assert.equal(server.listen(128), 0);
const socket = net.connect(address.port, '127.0.0.1');
socket.on('error', () => {});
socket.on('close', () => { assert.equal(accepts, 1); socketClosed = true; finish(); });
process.on('exit', () => {
  assert.equal(accepts, 1);
  assert.equal(closes, 2);
  assert.equal(socketClosed, true);
  assert.equal(reported, true);
});
