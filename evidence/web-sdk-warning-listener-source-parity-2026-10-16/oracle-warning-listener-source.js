'use strict';
const {spawnSync}=require('child_process');
const source="'use strict';\nconst descriptor=Object.getOwnPropertyDescriptor(process,'emitWarning');\nconst listener=process.listeners('warning')[0];\nconsole.log(JSON.stringify([Object.keys(descriptor),typeof descriptor.value,typeof descriptor.get,listener?.name,listener?.length,Object.hasOwn(listener,'prototype'),Function.prototype.toString.call(listener)]));\n";
const child=spawnSync(process.execPath,['-e',source],{encoding:'utf8',env:{...process.env,NODE_OPTIONS:'',NODE_NO_WARNINGS:''}});
process.stdout.write(child.stdout);process.stderr.write(child.stderr);process.exitCode=child.status===0?0:1;
