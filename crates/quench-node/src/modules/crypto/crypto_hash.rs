/// Compute a SHA-1 digest without relying on an external crypto crate.
const SHA1_INITIAL_H: [u32; 5] = [
    0x6745_2301,
    0xEFCD_AB89,
    0x98BA_DCFE,
    0x1032_5476,
    0xC3D2_E1F0,
];

fn sha1_compress(h: &mut [u32; 5], w: &mut [u32; 80]) {
    let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
    for (i, &wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(wi);
        (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
    }
    for (dst, src) in h.iter_mut().zip([a, b, c, d, e]) {
        *dst = dst.wrapping_add(src);
    }
}

fn sha1_finalize(h: [u32; 5]) -> [u8; 20] {
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Compute a SHA-1 digest without relying on an external crypto crate.
pub fn sha1_digest(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = SHA1_INITIAL_H;
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
        sha1_compress(&mut h, &mut w);
    }
    sha1_finalize(h)
}

/// SHA-256 round constants (first 32 bits of the cube roots of
/// the first 64 primes).
const SHA256_K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];
const SHA256_INITIAL_H: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

fn sha256_compress(h: &mut [u32; 8], w: &mut [u32; 64]) {
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut x) =
        (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = x
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        (x, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
    }
    for (dst, src) in h.iter_mut().zip([a, b, c, d, e, f, g, x]) {
        *dst = (*dst).wrapping_add(src);
    }
}

fn sha256_finalize(h: [u32; 8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, w) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
    }
    out
}

/// Compute a SHA-256 digest without relying on an external crypto crate.
pub fn sha256_digest(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = SHA256_INITIAL_H;
    let mut input = data.to_vec();
    input.push(0x80);
    while input.len() % 64 != 56 {
        input.push(0);
    }
    input.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in input.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, x) in w[..16].iter_mut().enumerate() {
            *x = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        sha256_compress(&mut h, &mut w);
    }
    sha256_finalize(h)
}
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];
const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];
fn md5_block(h: &mut [u32; 4], c: &[u8]) {
    let mut a = h[0];
    let mut b = h[1];
    let mut cc = h[2];
    let mut d = h[3];
    let mut m = [0u32; 16];
    for i in 0..16 {
        m[i] = u32::from_le_bytes(c[i * 4..i * 4 + 4].try_into().unwrap());
    }
    for i in 0..64 {
        let (f, g) = if i < 16 {
            ((b & cc) | (!b & d), i)
        } else if i < 32 {
            ((d & b) | (!d & cc), (5 * i + 1) % 16)
        } else if i < 48 {
            (b ^ cc ^ d, (3 * i + 5) % 16)
        } else {
            (cc ^ (b | !d), (7 * i) % 16)
        };
        let t = a
            .wrapping_add(f)
            .wrapping_add(MD5_K[i])
            .wrapping_add(m[g])
            .rotate_left(MD5_S[i]);
        a = d;
        d = cc;
        cc = b;
        b = b.wrapping_add(t);
    }
    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(cc);
    h[3] = h[3].wrapping_add(d);
}

pub fn md5_digest(data: &[u8]) -> [u8; 16] {
    let mut x = data.to_vec();
    x.push(0x80);
    while x.len() % 64 != 56 {
        x.push(0);
    }
    x.extend_from_slice(&((data.len() as u64) * 8).to_le_bytes());
    let mut h = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for c in x.chunks_exact(64) {
        md5_block(&mut h, c);
    }
    let mut out = [0u8; 16];
    for i in 0..4 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_le_bytes());
    }
    out
}
const SHA512_K: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];
const SHA512_INITIAL_H: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

fn sha512_compress(h: &mut [u64; 8], w: &mut [u64; 80]) {
    for i in 16..80 {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut x) =
        (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
    for i in 0..80 {
        let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = x
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA512_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        (x, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
    }
    for (dst, src) in h.iter_mut().zip([a, b, c, d, e, f, g, x]) {
        *dst = (*dst).wrapping_add(src);
    }
}

pub fn sha512_digest(data: &[u8]) -> [u8; 64] {
    let mut h: [u64; 8] = SHA512_INITIAL_H;
    let mut input = data.to_vec();
    input.push(0x80);
    while input.len() % 128 != 112 {
        input.push(0);
    }
    let bits = (data.len() as u128) * 8;
    input.extend_from_slice(&bits.to_be_bytes());
    for chunk in input.chunks_exact(128) {
        let mut w = [0u64; 80];
        for (i, word) in w[..16].iter_mut().enumerate() {
            *word = u64::from_be_bytes(chunk[i * 8..i * 8 + 8].try_into().unwrap());
        }
        sha512_compress(&mut h, &mut w);
    }
    let mut out = [0u8; 64];
    for (i, word) in h.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn sha384_digest(data: &[u8]) -> [u8; 48] {
    let full = sha512_digest(data);
    let mut out = [0u8; 48];
    out.copy_from_slice(&full[..48]);
    out
}
pub fn sha224_digest(data: &[u8]) -> [u8; 28] {
    let full = sha256_digest(data);
    let mut out = [0u8; 28];
    out.copy_from_slice(&full[..28]);
    out
}

fn hmac_with<F, const N: usize>(
    key: &[u8],
    data: &[u8],
    digest: F,
    block_size: usize,
) -> [u8; N]
where
    F: Fn(&[u8]) -> [u8; N],
{
    let mut k = key.to_vec();
    if k.len() > block_size {
        k = digest(&k).to_vec();
    }
    k.resize(block_size, 0);
    let mut inner = vec![0x36; block_size];
    let mut outer = vec![0x5c; block_size];
    for i in 0..block_size {
        inner[i] ^= k[i];
        outer[i] ^= k[i];
    }
    inner.extend_from_slice(data);
    let inner_digest = digest(&inner);
    outer.extend_from_slice(&inner_digest);
    digest(&outer)
}

pub fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    hmac_with(key, data, sha1_digest, 64)
}
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    hmac_with(key, data, sha256_digest, 64)
}
pub fn hmac_sha384(key: &[u8], data: &[u8]) -> [u8; 48] {
    hmac_with(key, data, sha384_digest, 128)
}
pub fn hmac_sha512(key: &[u8], data: &[u8]) -> [u8; 64] {
    hmac_with(key, data, sha512_digest, 128)
}
