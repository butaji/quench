'use strict';

const assert = require('node:assert/strict');
const { v4 } = require('uuid');

assert.match(v4(), /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);

