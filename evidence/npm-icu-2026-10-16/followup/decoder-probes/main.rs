mod text_decoder_icu;
use std::io::{self,BufRead};
fn main(){
 for line in io::stdin().lock().lines(){
  let v:serde_json::Value=serde_json::from_str(&line.unwrap()).unwrap();
  let result=match v["op"].as_str().unwrap(){
   "width"=>{let units=v["units"].as_array().unwrap().iter().map(|n|n.as_u64().unwrap() as u16).collect::<Vec<_>>();serde_json::json!(text_decoder_icu::string_width(&units,v["ambiguous"].as_bool().unwrap(),v["expand"].as_bool().unwrap()))},
   "transcode"=>{let bytes=v["bytes"].as_array().unwrap().iter().map(|n|n.as_u64().unwrap() as u8).collect::<Vec<_>>();match text_decoder_icu::transcode(&bytes,v["from"].as_str().unwrap(),v["to"].as_str().unwrap()) { Ok(b)=>serde_json::json!({"bytes":b}),Err(s)=>serde_json::json!({"error":text_decoder_icu::status_name(s),"errno":s}) }},
   _=>panic!()
  };
  println!("{}",result);
 }
}
