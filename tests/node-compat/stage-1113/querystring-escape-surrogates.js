const querystring = require("querystring");
const value = String.fromCharCode(0xd801) + "test";
if (querystring.escape(value) !== "%F0%90%91%B4est") {
  throw new Error("malformed surrogate encoding differs");
}
try {
  querystring.escape(String.fromCharCode(0xd801));
  throw new Error("lone surrogate was accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_URI") throw error;
}
