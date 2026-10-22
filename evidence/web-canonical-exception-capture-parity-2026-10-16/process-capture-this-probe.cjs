'use strict';
const p = require('process');
p.setUncaughtExceptionCaptureCallback(function (error) { console.log(JSON.stringify([this === p, Object.keys(this), this.captureFn === arguments.callee, error.message])); });
throw new Error('capture receiver');
