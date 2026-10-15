const b=require('internal/test/binding').internalBinding('encoding_binding');
for(const text of ['', 'abc', 'hé😀', '\ud800x\udc00', '😀a']) {
 console.log('encode',JSON.stringify(text),JSON.stringify([...b.encodeUtf8String(text)]));
 for(let n=0;n<8;n++){const full=new Uint8Array(n+4).fill(99),view=full.subarray(2,n+2);b.encodeInto(text,view);console.log('into',n,JSON.stringify([...b.encodeIntoResults]),JSON.stringify([...full]));}
}
for(const bytes of [[],[97],[239,187,191,97],[240,159,152,128],[237,160,128],[240,159],[192,175],[255]])for(const ignore of [false,true,1])for(const fatal of [false,true,1]){
 const whole=new Uint8Array([99,...bytes,99]),input=new DataView(whole.buffer,1,bytes.length);
 try{console.log('decode',JSON.stringify(bytes),ignore,fatal,JSON.stringify(b.decodeUTF8(input,ignore,fatal)))}catch(e){console.log('decode',JSON.stringify(bytes),ignore,fatal,e.name,e.code,e.message)}
}

const copy=require('internal/test/binding').internalBinding('buffer').copyArrayBuffer;
const bytes=new Uint8Array([0,1,2,3,4,5,6,7,8,9]);
Uint8Array.prototype.set=function(){throw Error('public set consulted')};
copy(bytes.buffer,2,bytes.buffer,0,6);console.log('overlap',JSON.stringify([...bytes]));
const target=new Uint8Array(10);copy(target.buffer,3,bytes.buffer,2,4);console.log('offset',JSON.stringify([...target]));
