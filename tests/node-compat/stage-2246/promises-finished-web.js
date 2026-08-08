const assert = require("assert");
const { finished } = require("stream/promises");
const { WritableStream } = require("stream/web");

const stream = new WritableStream({ write() {} });
const writer = stream.getWriter();
const completion = finished(stream);
writer.write("value");
writer.close();
completion.then(() => console.log("promise finished Web Stream passed"));

assert.rejects(
  finished(
    new ReadableStream({
      start(controller) {
        controller.error(new Error("boom"));
      }
    })
  ),
  /boom/
);
