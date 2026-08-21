const assert = require("assert");
const crypto = require("crypto");

const key = [
  "-----BEGIN RSA PRIVATE KEY-----",
  "MIGrAgEAAiEA+3z+1QNF2/unumadiwEr+C5vfhezsb3hp4jAnCNRpPcCAwEAAQIgQNriSQK4",
  "EFwczDhMZp2dvbcz7OUYt36z3S4usFPHSECEQD/41K7SujrstBfoCPzwC1xAhEA+5kt4BJy",
  "eKN7LggbF3Dk5wIQN6SL+fQ5H/+7NgARsVBp0QIRANxYRukavs4QvuyNhMx+vrkCEQCbf6j/",
  "Ig6/HueCK/0Jkmp+",
  "-----END RSA PRIVATE KEY-----",
].join("\n");

assert.throws(() => crypto.createSign("SHA256").update("test").sign(key), {
  message: "error:02000070:rsa routines::digest too big for rsa key",
  library: "rsa routines",
});
console.log("crypto RSA small-key error passed");
