const assert = require("assert");
const { WritableStream } = require("stream/web");
const { finished } = require("stream");

const stream = new WritableStream({ write() {} });
finished(stream, (error) => assert.ifError(error));
stream.getWriter().close();

const failed = new WritableStream({});
finished(failed, (error) => assert.strictEqual(error.message, "failed"));
failed.getWriter().abort(new Error("failed"));
