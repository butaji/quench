// Temporary measurement harness (not committed): live heap bytes per compiled pattern.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};
struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static TRACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static mut HIST: [isize; 64] = [0; 64];
fn rec(n: isize, sz: usize) { if TRACK.load(Relaxed) { let b = (usize::BITS - sz.leading_zeros()) as usize; unsafe { HIST[b] += n; } } }
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 { LIVE.fetch_add(l.size() as isize, Relaxed); rec(l.size() as isize, l.size()); unsafe { System.alloc(l) } }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { LIVE.fetch_sub(l.size() as isize, Relaxed); rec(-(l.size() as isize), l.size()); unsafe { System.dealloc(p, l) } }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 { LIVE.fetch_add(n as isize - l.size() as isize, Relaxed); rec(-(l.size() as isize), l.size()); rec(n as isize, n); unsafe { System.realloc(p, l, n) } }
}
#[global_allocator]
static A: Counting = Counting;
fn main() {
    let text = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    // patterns.json: [{"pattern":..,"flags":..}] — tiny ad-hoc parse via serde-free split
    let pats: Vec<(String, String)> = parse(&text);
    let subject = std::env::args().nth(2).unwrap_or_else(|| "uggc://jjj.snprobbx.pbz/ybtva.cuc?a=1&o=2 QBZPbageby_cynprubyqre pyvpx".into());
    let mut kept = Vec::new();
    let mut rows = Vec::new();
    let base = LIVE.load(Relaxed);
    for (p, f) in &pats {
        let before = LIVE.load(Relaxed);
        let Ok(r) = quench_regexp::Regex::with_flags(p, quench_regexp::Flags::from(f.as_str())) else { continue };
        let compiled = LIVE.load(Relaxed) - before;
        TRACK.store(true, Relaxed); let sub = quench_regexp::Subject::from_ascii(std::rc::Rc::from(subject.as_str()), 0..subject.len()); let _ = r.find_in_subject(&sub, 0); drop(sub); TRACK.store(false, Relaxed);
        if std::env::var_os("RX_WIDE").is_some() { let utf16: Vec<u16> = subject.encode_utf16().collect(); let _ = r.find_from_utf16(&utf16, 0); }
        let ran = LIVE.load(Relaxed) - before;
        rows.push((ran, compiled, p.clone()));
        kept.push(r);
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    for (ran, compiled, p) in rows.iter().take(25) { println!("{ran:8} {compiled:8} {}", &p[..p.len().min(70)]); }
    for b in 0..64 { let v = unsafe { HIST[b] }; if v != 0 { println!("run bucket <2^{b}: {} KiB", v / 1024); } }
    println!("sizeof Regex {}", std::mem::size_of::<quench_regexp::Regex>());
    println!("compiled sum {} KiB", rows.iter().map(|r| r.1).sum::<isize>() / 1024);
    println!("patterns {} total live {} KiB", kept.len(), (LIVE.load(Relaxed) - base) / 1024);
}
fn parse(t: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let v: Vec<&str> = t.split("{\"pattern\":").skip(1).collect();
    for item in v {
        let (p, rest) = json_str(item);
        let fi = rest.find("\"flags\":").unwrap() + 8;
        let (f, _) = json_str(&rest[fi..]);
        out.push((p, f));
    }
    out
}
fn json_str(s: &str) -> (String, &str) {
    let b = s.as_bytes(); assert_eq!(b[0], b'"');
    let mut i = 1; let mut o = String::new();
    while b[i] != b'"' {
        if b[i] == b'\\' { i += 1; match b[i] { b'n' => o.push('\n'), b't' => o.push('\t'), b'r' => o.push('\r'), b'u' => { let h = &s[i+1..i+5]; o.push(char::from_u32(u32::from_str_radix(h,16).unwrap()).unwrap_or('?')); i += 4; } c => o.push(c as char) } i += 1; continue; }
        let ch = s[i..].chars().next().unwrap(); o.push(ch); i += ch.len_utf8();
    }
    (o, &s[i + 1..])
}
