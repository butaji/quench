'use strict';
console.log(process.version,process.versions.node,process.version==='v'+process.versions.node);
const d=Object.getOwnPropertyDescriptor(process,'version');console.log(d.writable,d.enumerable,d.configurable);
const v=Object.getOwnPropertyDescriptor(process.versions,'node');console.log(v.writable,v.enumerable,v.configurable);
