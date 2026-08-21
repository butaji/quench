//! Partial `node:crypto` surface.
use std::cell::RefCell;
use std::rc::Rc;
use quench_runtime::{execute, host_api};
use quench_runtime::value::Value;
use crate::host::HostState;

pub fn build() -> Value {
    host_api::object(vec![
        ("randomBytes".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_BYTES)),
        ("randomFillSync".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_FILL_SYNC)),
        ("createHash".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_HASH)),
        ("createHmac".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_HMAC)),
        ("timingSafeEqual".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_TIMING_SAFE_EQUAL)),
        ("randomUUID".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_UUID)),
        ("randomInt".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_INT)),
        ("getHashes".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_GET_HASHES)),
        ("getCiphers".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_GET_CIPHERS)),
        ("constants".into(), host_api::object(vec![
            ("OPENSSL_VERSION_NUMBER".into(), Value::Number(0.0)),
            ("defaultCoreCipherList".into(), Value::String("".into())),
        ])),
        ("createCipheriv".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_UNSUPPORTED)),
        ("createDecipheriv".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_UNSUPPORTED)),
        ("generateKeyPairSync".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_UNSUPPORTED)),
    ])
}

fn random_into(bytes: &mut [u8]) -> Result<(), quench_runtime::execute::VmError> {
    #[cfg(unix)]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .map_err(|e| quench_runtime::execute::VmError::EvalError(format!("random source unavailable: {e}")))?
            .read_exact(bytes)
            .map_err(|e| quench_runtime::execute::VmError::EvalError(format!("random source unavailable: {e}")))?;
        Ok(())
    }
    #[cfg(not(unix))]
    { Err(quench_runtime::execute::VmError::EvalError("randomBytes is unsupported on this platform".into())) }
}


pub fn random_bytes(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    let n = match args.first() {
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 && *n <= 2_147_483_647.0 => *n as usize,
        _ => return Err(execute::type_error("The \"size\" argument must be of type number.")),
    };
    let mut bytes = vec![0u8; n];
    random_into(&mut bytes)?;
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}

pub fn random_fill_sync(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    let view = match args.first() {
        Some(Value::Uint8Array(view)) => view,
        _ => return Err(execute::type_error("The \"buffer\" argument must be an instance of Buffer or Uint8Array.")),
    };
    let offset = match args.get(1) {
        None | Some(Value::Undefined) => 0,
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => *n as usize,
        _ => return Err(execute::type_error("The \"offset\" argument must be of type number.")),
    };
    let size = match args.get(2) {
        None | Some(Value::Undefined) => view.length.saturating_sub(offset),
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => *n as usize,
        _ => return Err(execute::type_error("The \"size\" argument must be of type number.")),
    };
    if offset.checked_add(size).is_none() || offset + size > view.length {
        return Err(execute::type_error("The value of \"offset\" is out of range."));
    }
    let start = view.byte_offset + offset;
    let end = start + size;
    random_into(&mut view.buffer.bytes.borrow_mut()[start..end])?;
    Ok(Value::Uint8Array(view.clone()))
}

pub fn unsupported(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    Err(quench_runtime::execute::VmError::EvalError("node:crypto operation is not supported by quench".into()))
}
fn argbytes(v:&Value)->Result<Vec<u8>,quench_runtime::execute::VmError>{match v{Value::Uint8Array(x)=>Ok(x.buffer.bytes.borrow()[x.byte_offset..x.byte_offset+x.length].to_vec()),Value::String(s)=>Ok(s.as_bytes().to_vec()),_=>
Err(execute::type_error("argument must be a Buffer or string"))}}
pub fn timing_safe_equal(_: &Rc<RefCell<HostState>>,a:&[Value])->Result<Value,quench_runtime::execute::VmError>{let x=argbytes(a.first().ok_or_else(||execute::type_error("missing argument"))?)?;let y=argbytes(a.get(1).ok_or_else(||execute::type_error("missing argument"))?)?;if x.len()!=y.len(){return Err(execute::type_error("Input buffers must have the same byte length"))}let mut d=0;for(i,j)in x.iter().zip(y.iter()){d|=i^j}Ok(Value::Boolean(d==0))}
pub fn random_uuid(_: &Rc<RefCell<HostState>>,_:&[Value])->Result<Value,quench_runtime::execute::VmError>{let mut b=[0u8;16];random_into(&mut b)?;b[6]=(b[6]&15)|64;b[8]=(b[8]&63)|128;Ok(Value::String(format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",b[0],b[1],b[2],b[3],b[4],b[5],b[6],b[7],b[8],b[9],b[10],b[11],b[12],b[13],b[14],b[15])))}
pub fn random_int(_: &Rc<RefCell<HostState>>,a:&[Value])->Result<Value,quench_runtime::execute::VmError>{let min=match a.first(){Some(Value::Number(n))=>*n as i64,_=>0};let max=match a.last(){Some(Value::Number(n))=>*n as i64,_=>return Err(execute::type_error("max must be a number"))};if max<=min{return Err(execute::VmError::EvalError("max must be greater than min".into()))}let mut b=[0;8];random_into(&mut b)?;Ok(Value::Number((min+(u64::from_ne_bytes(b)%(max-min)as u64) as i64)as f64))}
pub fn get_hashes(_: &Rc<RefCell<HostState>>,_:&[Value])->Result<Value,quench_runtime::execute::VmError>{Ok(host_api::array(vec![Value::String("sha1".into()),Value::String("sha256".into())]))}
const CRYPTO_ALG: &str = "\0crypto:alg";
const CRYPTO_DATA: &str = "\0crypto:data";
const CRYPTO_KEY: &str = "\0crypto:key";

pub fn create_hash(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    let alg = match args.first() { Some(Value::String(s)) => s.to_lowercase(), _ => return Err(execute::type_error("algorithm required")) };
    if alg != "sha1" && alg != "sha-1" && alg != "sha256" && alg != "sha-256" { return Err(execute::type_error("Digest method not supported")); }
    let mut object = host_api::object(vec![]);
    object = execute::set_property(object, CRYPTO_ALG, Value::String(alg));
    object = execute::set_property(object, CRYPTO_DATA, crate::modules::buffer_proto::make_buffer(&[]));
    object = execute::set_property(object, "update", crate::host::capability(crate::registry::NodeSpec::new("crypto:hashUpdate", 0x210A)));
    object = execute::set_property(object, "digest", crate::host::capability(crate::registry::NodeSpec::new("crypto:hashDigest", 0x210B)));
    Ok(object)
}

pub fn create_hmac(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    let alg = match args.first() { Some(Value::String(s)) => s.to_lowercase(), _ => return Err(execute::type_error("algorithm required")) };
    let key = argbytes(args.get(1).ok_or_else(|| execute::type_error("key required"))?)?;
    let mut object = host_api::object(vec![]);
    object = execute::set_property(object, CRYPTO_ALG, Value::String(alg));
    object = execute::set_property(object, CRYPTO_KEY, crate::modules::buffer_proto::make_buffer(&key));
    object = execute::set_property(object, CRYPTO_DATA, crate::modules::buffer_proto::make_buffer(&[]));
    object = execute::set_property(object, "update", crate::host::capability(crate::registry::NodeSpec::new("crypto:hmacUpdate", 0x210C)));
    object = execute::set_property(object, "digest", crate::host::capability(crate::registry::NodeSpec::new("crypto:hmacDigest", 0x210D)));
    Ok(object)
}

pub fn hash_update(_: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    let object = receiver.ok_or_else(|| execute::type_error("receiver"))?;
    let mut data = argbytes(&execute::get_property(object, CRYPTO_DATA))?;
    data.extend(argbytes(args.first().ok_or_else(|| execute::type_error("data required"))?)?);
    Ok(execute::set_property(object.clone(), CRYPTO_DATA, crate::modules::buffer_proto::make_buffer(&data)))
}

pub fn hash_digest(_: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    let object = receiver.ok_or_else(|| execute::type_error("receiver"))?;
    let data = argbytes(&execute::get_property(object, CRYPTO_DATA))?;
    let alg = execute::get_property(object, CRYPTO_ALG);
    let bytes = if let Value::String(name) = alg { if name.contains("1") { sha1_digest(&data).to_vec() } else { sha256_digest(&data).to_vec() } } else { sha256_digest(&data).to_vec() };
    if matches!(args.first(), Some(Value::String(s)) if s == "hex") { return Ok(Value::String(hex::encode(bytes))); }
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}
/// Compute a SHA-1 digest without relying on an external crypto crate.
pub fn sha1_digest(data: &[u8]) -> [u8; 20] {
    let mut h = [
        0x6745_2301u32, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0,
    ];
    let mut input = data.to_vec();
    input.push(0x80);
    while input.len() % 64 != 56 {
        input.push(0);
    }
    input.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in input.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w[..16].iter_mut().enumerate() {
            *word = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(wi);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (dst, src) in h.iter_mut().zip([a, b, c, d, e]) {
            *dst = dst.wrapping_add(src);
        }
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Compute a SHA-256 digest without relying on an external crypto crate.
pub fn sha256_digest(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98,0x7137_4491,0xb5c0_fbcf,0xe9b5_dba5,0x3956_c25b,0x59f1_11f1,0x923f_82a4,0xab1c_5ed5,
        0xd807_aa98,0x1283_5b01,0x2431_85be,0x550c_7dc3,0x72be_5d74,0x80de_b1fe,0x9bdc_06a7,0xc19b_f174,
        0xe49b_69c1,0xefbe_4786,0x0fc1_9dc6,0x240c_a1cc,0x2de9_2c6f,0x4a74_84aa,0x5cb0_a9dc,0x76f9_88da,
        0x983e_5152,0xa831_c66d,0xb003_27c8,0xbf59_7fc7,0xc6e0_0bf3,0xd5a7_9147,0x06ca_6351,0x1429_2967,
        0x27b7_0a85,0x2e1b_2138,0x4d2c_6dfc,0x5338_0d13,0x650a_7354,0x766a_0abb,0x81c2_c92e,0x9272_2c85,
        0xa2bf_e8a1,0xa81a_664b,0xc24b_8b70,0xc76c_51a3,0xd192_e819,0xd699_0624,0xf40e_3585,0x106a_a070,
        0x19a4_c116,0x1e37_6c08,0x2748_774c,0x34b0_bcb5,0x391c_0cb3,0x4ed8_aa4a,0x5b9c_ca4f,0x682e_6ff3,
        0x748f_82ee,0x78a5_636f,0x84c8_7814,0x8cc7_0208,0x90be_fffa,0xa450_6ceb,0xbef9_a3f7,0xc671_78f2,
    ];
    let mut h: [u32; 8] = [0x6a09_e667,0xbb67_ae85,0x3c6e_f372,0xa54f_f53a,0x510e_527f,0x9b05_688c,0x1f83_d9ab,0x5be0_cd19];
    let mut input = data.to_vec(); input.push(0x80);
    while input.len() % 64 != 56 { input.push(0); }
    input.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in input.chunks_exact(64) {
        let mut w=[0u32;64];
        for (i,x) in w[..16].iter_mut().enumerate(){*x=u32::from_be_bytes(chunk[i*4..i*4+4].try_into().unwrap());}
        for i in 16..64 { let s0=w[i-15].rotate_right(7)^w[i-15].rotate_right(18)^(w[i-15]>>3); let s1=w[i-2].rotate_right(17)^w[i-2].rotate_right(19)^(w[i-2]>>10); w[i]=w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1); }
        let (mut a,mut b,mut c,mut d,mut e,mut f,mut g,mut x)= (h[0],h[1],h[2],h[3],h[4],h[5],h[6],h[7]);
        for i in 0..64 { let s1=e.rotate_right(6)^e.rotate_right(11)^e.rotate_right(25); let ch=(e&f)^((!e)&g); let t1=x.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]); let s0=a.rotate_right(2)^a.rotate_right(13)^a.rotate_right(22); let maj=(a&b)^(a&c)^(b&c); let t2=s0.wrapping_add(maj); (x,g,f,e,d,c,b,a)=(g,f,e,d.wrapping_add(t1),c,b,a,t1.wrapping_add(t2)); }
        for (dst,src) in h.iter_mut().zip([a,b,c,d,e,f,g,x]) {*dst=(*dst).wrapping_add(src);}
    }
    let mut out=[0u8;32]; for(i,w)in h.iter().enumerate(){out[i*4..i*4+4].copy_from_slice(&w.to_be_bytes());} out
}

fn hmac_with<F, const N: usize>(key: &[u8], data: &[u8], digest: F) -> [u8; N]
where F: Fn(&[u8]) -> [u8; N] {
    let mut k = key.to_vec();
    if k.len() > 64 { k = digest(&k).to_vec(); }
    k.resize(64, 0);
    let mut inner = vec![0x36; 64]; let mut outer = vec![0x5c; 64];
    for i in 0..64 { inner[i] ^= k[i]; outer[i] ^= k[i]; }
    inner.extend_from_slice(data); let inner_digest = digest(&inner);
    outer.extend_from_slice(&inner_digest); digest(&outer)
}

pub fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] { hmac_with(key, data, sha1_digest) }
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] { hmac_with(key, data, sha256_digest) }
