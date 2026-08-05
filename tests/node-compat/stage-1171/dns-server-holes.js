const assert = require("assert");
const dns = require("dns");

const servers = [];
servers[0] = "127.0.0.1";
servers[2] = "0.0.0.0";
dns.setServers(servers);
assert.deepStrictEqual(dns.getServers(), ["127.0.0.1", "0.0.0.0"]);
