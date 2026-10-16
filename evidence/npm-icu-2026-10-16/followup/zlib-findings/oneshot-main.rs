#[path="native-codec.rs"]mod codec;
use std::io::{self,BufRead};
fn main(){for line in io::stdin().lock().lines(){
 let r:serde_json::Value=serde_json::from_str(&line.unwrap()).unwrap();
 let h=match r["mode"].as_str().unwrap(){"gzip"=>codec::Header::Gzip,"deflate"=>codec::Header::Zlib,"deflateRaw"=>codec::Header::Raw,_=>panic!()};
 let input=r["bytes"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap()as u8).collect::<Vec<_>>();
 let output=codec::compress(h,&input,r["level"].as_i64().unwrap()as i32).unwrap();println!("{}",serde_json::json!(output));
}}
