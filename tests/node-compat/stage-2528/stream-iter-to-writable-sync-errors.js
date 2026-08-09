const assert = require("assert");
const { toWritable } = require("stream/iter");

(async () => {
  let syncWrites = 0;
  let asyncWrites = 0;
  const writer = {
    writeSync() {
      syncWrites++;
      return true;
    },
    write() {
      asyncWrites++;
      return Promise.resolve();
    },
    endSync() {
      return 1;
    },
    end() {
      throw new Error("end should not run");
    },
    fail(error) {
      assert.strictEqual(error.message, "boom");
    }
  };
  const writable = toWritable(writer);
  await new Promise((resolve, reject) =>
    writable.write("x", (error) => (error ? reject(error) : resolve()))
  );
  assert.strictEqual(syncWrites, 1);
  assert.strictEqual(asyncWrites, 0);
  await new Promise((resolve, reject) =>
    writable.end((error) => (error ? reject(error) : resolve()))
  );

  const failing = toWritable({
    write() {
      throw new Error("write failed");
    }
  });
  await new Promise((resolve) =>
    failing.write("x", (error) => {
      assert.strictEqual(error.message, "write failed");
      resolve();
    })
  );

  const destroyable = toWritable({
    write() {
      return Promise.resolve();
    },
    fail(error) {
      assert.strictEqual(error.message, "boom");
    }
  });
  destroyable.on("error", () => {});
  destroyable.destroy(new Error("boom"));
  await new Promise((resolve) => setTimeout(resolve, 0));
})();
