use std::{ffi::{c_char,c_int,c_uint,c_ulong,c_void},io::{self,BufRead}};
#[repr(C)]
struct Stream{
 next_in:*mut u8,avail_in:c_uint,total_in:c_ulong,
 next_out:*mut u8,avail_out:c_uint,total_out:c_ulong,
 msg:*mut c_char,state:*mut c_void,
 zalloc:Option<unsafe extern "C" fn(*mut c_void,c_uint,c_uint)->*mut c_void>,
 zfree:Option<unsafe extern "C" fn(*mut c_void,*mut c_void)>,opaque:*mut c_void,
 data_type:c_int,adler:c_ulong,reserved:c_ulong,
}
#[link(name="nodezlib",kind="static")]
unsafe extern "C"{
 fn z_zlibVersion()->*const c_char;
 fn z_deflateInit2_(stream:*mut Stream,level:c_int,method:c_int,window:c_int,memory:c_int,strategy:c_int,version:*const c_char,size:c_int)->c_int;
 fn z_deflate(stream:*mut Stream,flush:c_int)->c_int;
 fn z_deflateEnd(stream:*mut Stream)->c_int;
}
struct Compressor(Box<Stream>);
impl Drop for Compressor{fn drop(&mut self){unsafe{z_deflateEnd(&mut*self.0);}}}
fn main(){
 for line in io::stdin().lock().lines(){
  let row:serde_json::Value=serde_json::from_str(&line.unwrap()).unwrap();
  let window=match row["mode"].as_str().unwrap(){"Deflate"=>15,"DeflateRaw"=>-15,"Gzip"=>31,_=>panic!()};
  let mut raw=Box::new(unsafe{std::mem::zeroed::<Stream>()});
  let status=unsafe{z_deflateInit2_(&mut*raw,row["level"].as_u64().unwrap() as i32,8,window,8,0,z_zlibVersion(),std::mem::size_of::<Stream>() as i32)};assert_eq!(status,0);
  let mut stream=Compressor(raw);let mut chunks=Vec::new();
  for chunk in row["chunks"].as_array().unwrap(){
   let input=chunk["bytes"].as_array().unwrap().iter().map(|n|n.as_u64().unwrap() as u8).collect::<Vec<_>>();
   let mut offset=0;let mut output=Vec::new();
   loop{
    let mut buffer=[0u8;64];let state=&mut*stream.0;
    state.next_in=unsafe{input.as_ptr().add(offset)}.cast_mut();state.avail_in=(input.len()-offset) as u32;
    state.next_out=buffer.as_mut_ptr();state.avail_out=buffer.len() as u32;
    let before=state.avail_in;let status=unsafe{z_deflate(state,chunk["flush"].as_u64().unwrap() as i32)};assert!([0,1,-5].contains(&status),"zlib status {status}");
    offset+=(before-state.avail_in) as usize;let written=buffer.len()-state.avail_out as usize;output.extend_from_slice(&buffer[..written]);
    if written<buffer.len(){break;}
   }
   chunks.push(serde_json::json!({"bytes":output,"consumed":offset}));
  }
  println!("{}",serde_json::json!(chunks));
 }
}
