'use strict';
const p = require('process');
p.setUncaughtExceptionCaptureCallback(function capture(error) { console.log(JSON.stringify([this === p, Object.keys(this), this.captureFn === capture, error.message])); });
throw new Error('capture receiver');
