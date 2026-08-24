"use strict";

const assert = require("assert");
const { Channel } = require("diagnostics_channel");
const diagnostics = require("diagnostics_channel");

const channel = diagnostics.channel("lexical-isolation");
assert.ok(channel instanceof Channel);
assert.strictEqual(channel.name, "lexical-isolation");
