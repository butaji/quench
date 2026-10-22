import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;const checks={};
  const equal=(id,actual,expected)=>{assert.deepEqual(actual,expected);assertions++;checks[id]=(checks[id]??0)+1;};
  for(const [value,bytes] of [[0,[0,0,0,0]],[-0,[0,0,0,128]],[1,[0,0,128,63]],[-2,[0,0,0,192]],[Infinity,[0,0,128,127]],[-Infinity,[0,0,128,255]],[2**-149,[1,0,0,0]]]) for(const little of [true,false]) {
    const expected=little?bytes:[...bytes].reverse(), buffer=new Uint8Array(6).fill(0xaa);
    equal('write',api.write(buffer,value,1,little,23,4),undefined);
    equal('write',Array.from(buffer),[0xaa,...expected,0xaa]);
    equal('read',api.read(buffer,1,little,23,4),value);
  }
  for(const [mantissa,length] of [[23,4],[52,8]]) for(const little of [true,false]) for(const value of [Math.PI,1e-30,1e30,NaN]) {
    const expected=new Uint8Array(length), view=new DataView(expected.buffer), actual=new Uint8Array(length);
    if(length===4)view.setFloat32(0,value,little);else view.setFloat64(0,value,little);
    api.write(actual,value,0,little,mantissa,length);
    if(!Number.isNaN(value))equal('write',Array.from(actual),Array.from(expected));
    equal('read',api.read(actual,0,little,mantissa,length),length===4?Math.fround(value):value);
  }
  return {package:'ieee754',assertions,checks,scope:'Finite endian, offset, binary32/64, subnormal and special-value corpus; arbitrary encodings remain obligations'};
}
