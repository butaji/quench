'use strict';
const assert=require('assert');
const api=require('node:test');
const events=[];
const object={method(){return 'original'}};
const original=object.method;
api.before(()=>events.push('before'));
api.beforeEach(t=>events.push('beforeEach:'+t.name));
api.afterEach(t=>events.push('afterEach:'+t.name));
api.after(()=>{
  assert.strictEqual(object.method,original);
  events.push('after');
  console.log(JSON.stringify(events));
});
api.test('parent',async t=>{
  assert.strictEqual(api.getTestContext(),t);
  events.push('parent');
  t.mock.method(object,'method',()=> 'mocked');
  await t.test('child',async child=>{
    assert.strictEqual(api.getTestContext(),child);
    await Promise.resolve();
    assert.strictEqual(api.getTestContext(),child);
    assert.strictEqual(object.method(),'mocked');
    events.push('child');
  });
  assert.strictEqual(api.getTestContext(),t);
  events.push('parent-end');
});
api.test.skip('skipped',()=>{throw new Error('skipped body executed')});
api.test.todo('pending');
