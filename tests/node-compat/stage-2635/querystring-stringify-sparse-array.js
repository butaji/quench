"use strict";

const assert = require("assert");
const querystring = require("node:querystring");

const values = [];
values.length = 3;
values[2] = "tail";
assert.strictEqual(querystring.stringify({ values }), "values=&values=&values=tail");

console.log("querystring sparse stringify passed");
