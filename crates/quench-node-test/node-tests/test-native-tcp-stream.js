'use strict';
const assert = require('assert');
const net = require('net');
const { internalBinding } = require('internal/test/binding');
const { TCP } = internalBinding('tcp_wrap');
const binding = internalBinding('stream_wrap');
for (const [name, index] of [['kReadBytesOrError', 0], ['kArrayBufferOffset', 1], ['kBytesWritten', 2], ['kLastWriteWasAsync', 3], ['kLastWriteErr', 4]]) assert.equal(binding[name], index);
assert(binding.streamBaseState instanceof Int32Array);
assert.equal(binding.streamBaseState.length, 5);
const listener = new TCP(1);
assert.equal(listener.bind('127.0.0.1', 0), 0);
const address = {};
assert.equal(listener.getsockname(address), 0);
let reads = 0, eof = 0, closed = 0;
let received = '', echoed = '';
listener.onconnection = function(status, handle) {
  assert.equal(status, 0);
  assert.strictEqual(this, listener);
  handle.onread = function(arrayBuffer) {
    assert.strictEqual(this, handle);
    const nread = binding.streamBaseState[binding.kReadBytesOrError];
    const offset = binding.streamBaseState[binding.kArrayBufferOffset];
    if (nread > 0) {
      reads++;
      assert(arrayBuffer instanceof ArrayBuffer);
      const bytes = Buffer.from(arrayBuffer, offset, nread);
      received += bytes.toString();
      assert.equal(received, 'hello world'.slice(0, received.length));
      const request = new binding.WriteWrap();
      request.async = false;
      request.oncomplete = () => assert.fail('synchronous write must not invoke oncomplete');
      assert.equal(handle.writeBuffer(request, bytes), 0);
      assert.equal(handle.writeQueueSize, 0);
      assert.equal(binding.streamBaseState[binding.kBytesWritten], nread);
      assert.equal(binding.streamBaseState[binding.kLastWriteWasAsync], 0);
      assert.equal(binding.streamBaseState[binding.kLastWriteErr], 0);
      assert.equal(request.async, false);
    } else {
      eof++;
      assert.equal(arrayBuffer, undefined);
      assert.equal(nread, -4095);
      assert.equal(received, 'hello world');
      console.log('intrinsic ArrayBuffer onread; real synchronous native write; state updated; no completion request');
      handle.close(() => { closed++; console.log('EOF state=-4095; native close complete'); });
      listener.close();
    }
  };
  assert.equal(handle.readStart(), 0);
};
assert.equal(listener.listen(128), 0);
const socket = net.connect(address.port, '127.0.0.1');
socket.on('connect', () => socket.end('hello world'));
socket.on('data', bytes => {
  echoed += bytes.toString();
  assert.equal(echoed, 'hello world'.slice(0, echoed.length));
});
socket.on('error', error => { throw error; });
process.on('exit', () => {
  assert(reads >= 1);
  assert.equal(received, 'hello world');
  assert.equal(echoed, 'hello world');
  assert.equal(eof, 1);
  assert.equal(closed, 1);
});
