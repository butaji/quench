"use strict";
const assert = require("assert");
const { AsyncLocalStorage } = require("async_hooks");

const als = new AsyncLocalStorage();

async function asyncFunctionAfterAwait() {
  await 0;
  als.enterWith("after await");
}

function promiseThen() {
  return Promise.resolve().then(() => {
    als.enterWith("inside then");
  });
}

async function asyncFunctionBeforeAwait() {
  als.enterWith("before await");
  await 0;
}

async function main() {
  await asyncFunctionAfterAwait();
  await promiseThen();
  assert.strictEqual(als.getStore(), undefined);
  await asyncFunctionBeforeAwait();
  assert.strictEqual(als.getStore(), "before await");
}

main().then(
  () => {},
  (error) => {
    throw error;
  },
);
