import assert from 'node:assert/strict';
import {Readable,Writable,Duplex,Transform,PassThrough} from 'node:stream';
export function exercise(api) {
  let assertions=0;const checks={};
  const names=['isStream','isReadableStream','isWritableStream','isDuplexStream','isTransformStream'];
  const equal=(id,actual,expected)=>{assert.deepEqual(actual,expected);assertions++;checks[id]=(checks[id]??0)+1;};
  for(const input of [null,undefined,0,'x',{},()=>{}])for(const name of names)equal(name,api[name](input),false);
  const readable=new Readable({read(){}}),writable=new Writable({write(chunk,encoding,callback){callback();}}),duplex=new Duplex({read(){},write(chunk,encoding,callback){callback();}}),transform=new Transform({transform(chunk,encoding,callback){callback(null,chunk);}}),pass=new PassThrough();
  for(const [input,expected] of [[readable,[true,true,false,false,false]],[writable,[true,false,true,false,false]],[duplex,[true,true,true,true,false]],[transform,[true,true,true,true,true]],[pass,[true,true,true,true,true]]]) {
    for(let index=0;index<names.length;index++)equal(names[index],api[names[index]](input),expected[index]);
    input.destroy();
    for(let index=0;index<names.length;index++)equal(names[index],api[names[index]](input,{checkOpen:false}),expected[index]);
  }
  for(const name of names)equal(name,api[name]({pipe(){},readable:false,writable:false}),false);
  equal('isStream',api.isStream({pipe(){}}),true);
  return {package:'is-stream',assertions,checks,scope:'Finite native stream classes, closed/open policy and invalid receivers corpus; arbitrary duck types and lifecycle interleavings remain obligations'};
}
