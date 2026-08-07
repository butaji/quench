"use strict";

const assert = require("assert");
const readline = require("readline/promises");

const listeners = {};
const input = {
  once(event, callback) {
    listeners[event] = callback;
  },
  pause() {
    input.paused = true;
  },
};
const output = {
  write(value) {
    output.prompt = value;
  },
};
const interfaceApi = readline.createInterface({ input, output });
const answer = interfaceApi.question("name? ");
listeners.line("quench");

(async () => {
  assert.strictEqual(await answer, "quench");
  assert.strictEqual(output.prompt, "name? ");
  interfaceApi.close();
  assert.strictEqual(input.paused, true);
  console.log("readline promises passed");
})();
