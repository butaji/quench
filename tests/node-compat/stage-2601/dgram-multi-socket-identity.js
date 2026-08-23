"use strict";

const assert = require("assert");
const dgram = require("dgram");

const first = dgram.createSocket("udp4");
const second = dgram.createSocket("udp4");
let closed = 0;
first.on("close", () => { closed += 1; });
second.on("close", () => { closed += 1; });

first.bind(0, "127.0.0.1", () => {
  second.bind(0, "127.0.0.1", () => {
    const firstPort = first.address().port;
    const secondPort = second.address().port;
    assert.notStrictEqual(firstPort, secondPort);
    assert.strictEqual(first.address().address, "127.0.0.1");
    assert.strictEqual(second.address().address, "127.0.0.1");
    first.send(Buffer.from("one"), secondPort, "127.0.0.1", (error, bytes) => {
      assert.ifError(error);
      assert.strictEqual(bytes, 3);
      second.send(Buffer.from("two"), firstPort, "127.0.0.1", (error2, bytes2) => {
        assert.ifError(error2);
        assert.strictEqual(bytes2, 3);
        first.close();
        second.close();
        assert.strictEqual(closed, 2);
        console.log("dgram multi-socket identity passed");
      });
    });
  });
});
