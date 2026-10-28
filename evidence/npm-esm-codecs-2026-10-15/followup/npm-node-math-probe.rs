mod pinned_cbrt { include!("npm-node-cbrt.rs"); }
use std::io::{self,BufRead};
fn main(){for line in io::stdin().lock().lines(){let line=line.unwrap();let fields:Vec<_>=line.split_whitespace().collect();let x=f64::from_bits(u64::from_str_radix(fields[1],16).unwrap());let result=match fields[0]{"cbrt"=>pinned_cbrt::cbrt(x),"atan2"=>{let y=f64::from_bits(u64::from_str_radix(fields[2],16).unwrap());pinned_cbrt::atan2(x,y)},_=>panic!()};println!("{:016x}",result.to_bits());}}
