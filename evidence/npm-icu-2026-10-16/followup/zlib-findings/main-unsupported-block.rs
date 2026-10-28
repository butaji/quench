use flate2::{Compress,Compression,FlushCompress,Status};
use std::io::{self,BufRead};
fn main(){
 for line in io::stdin().lock().lines(){
  let row:serde_json::Value=serde_json::from_str(&line.unwrap()).unwrap();
  let level=Compression::new(row["level"].as_u64().unwrap() as u32);
  let mut stream=match row["mode"].as_str().unwrap(){"Deflate"=>Compress::new(level,true),"DeflateRaw"=>Compress::new(level,false),"Gzip"=>Compress::new_gzip(level,15),_=>panic!()};
  let mut chunks=Vec::new();
  for chunk in row["chunks"].as_array().unwrap(){
   let input=chunk["bytes"].as_array().unwrap().iter().map(|n|n.as_u64().unwrap() as u8).collect::<Vec<_>>();
   let flush=match chunk["flush"].as_u64().unwrap(){0=>FlushCompress::None,1=>FlushCompress::Partial,2=>FlushCompress::Sync,3=>FlushCompress::Full,4=>FlushCompress::Finish,5=>FlushCompress::Block,_=>panic!()};
   let mut offset=0;let mut output=Vec::new();
   loop{
    let mut buffer=[0u8;64];let before_in=stream.total_in();let before_out=stream.total_out();
    let status=stream.compress(&input[offset..],&mut buffer,flush).unwrap();
    let consumed=(stream.total_in()-before_in) as usize;let written=(stream.total_out()-before_out) as usize;
    offset+=consumed;output.extend_from_slice(&buffer[..written]);
    if written<buffer.len()||status==Status::StreamEnd{break;}
   }
   chunks.push(serde_json::json!({"bytes":output,"consumed":offset}));
  }
  println!("{}",serde_json::json!(chunks));
 }
}
