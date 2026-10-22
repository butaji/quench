'use strict';
const assert=require('assert'),hooks=require('async_hooks'),events=[];let target;
function original(){throw new Error('original callback must have been replaced by the init hook')}
function replacement(a,b){'use strict';assert.strictEqual(this,undefined);assert.strictEqual(hooks.executionAsyncId(),target.id);assert.strictEqual(hooks.executionAsyncResource(),target.resource);events.push(['replacement',a,b]);}
const hook=hooks.createHook({
 init(id,type,trigger,resource){if(type==='TickObject'&&resource.callback===original){target={id,trigger,resource};resource.callback=replacement;resource.args=[4,5];events.push('init');}},
 before(id){if(target&&id===target.id){assert.strictEqual(hooks.triggerAsyncId(),target.trigger);events.push('before');}},
 after(id){if(target&&id===target.id){assert.strictEqual(hooks.executionAsyncResource(),target.resource);events.push('after');}},
 destroy(id){if(target&&id===target.id)events.push('destroy');}
}).enable();
process.nextTick(original,1,2);
setImmediate(()=>{hook.disable();assert.deepStrictEqual(events,['init','before',['replacement',4,5],'after','destroy']);console.log(JSON.stringify(events));});
