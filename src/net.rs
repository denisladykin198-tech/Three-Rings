//! Мини-стек: HTTP-рукопожатие + WebSocket (RFC 6455) на голом std.
use std::collections::VecDeque;
use std::io::{self, BufRead, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const MAX_HEADER_LINE: usize = 8 * 1024;
const MAX_HEADERS: usize = 64;
pub const MAX_FRAME: usize = 1024 * 1024;
pub const MAX_MESSAGE: usize = 256 * 1024;
const MAX_QUEUE_BYTES: usize = 8 * 1024 * 1024;
const PING_EVERY: Duration = Duration::from_secs(15);

// ------------------------------------------------------------------ SHA-1
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let bits = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bits.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a.rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d; d = c; c = b.rotate_left(30); b = a; a = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for i in 0..5 {
        out[4 * i..4 * i + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

// ------------------------------------------------------------------ base64
pub fn b64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity((data.len() + 2) / 3 * 4);
    for c in data.chunks(3) {
        let b0 = c[0] as u32;
        let b1 = *c.get(1).unwrap_or(&0) as u32;
        let b2 = *c.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        s.push(A[((n >> 18) & 63) as usize] as char);
        s.push(A[((n >> 12) & 63) as usize] as char);
        s.push(if c.len() > 1 { A[((n >> 6) & 63) as usize] as char } else { '=' });
        s.push(if c.len() > 2 { A[(n & 63) as usize] as char } else { '=' });
    }
    s
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

// ------------------------------------------------------------------ HTTP
pub enum Req { Page(String), Ws(String) }

// ------------------------------------------------------------------ HTTP
fn read_line_limited(rd: &mut impl BufRead, limit: usize) -> io::Result<Option<String>> {
    let mut buf = Vec::new();
    loop {
        let (done, used) = {
            let avail = match rd.fill_buf() {
                Ok(a) => a,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if avail.is_empty() {
                (true, 0)
            } else {
                match avail.iter().position(|&b| b == b'\n') {
                    Some(i) => { buf.extend_from_slice(&avail[..=i]); (true, i + 1) }
                    None => { buf.extend_from_slice(avail); (false, avail.len()) }
                }
            }
        };
        rd.consume(used);
        if buf.len() > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "заголовок слишком длинный"));
        }
        if done {
            if buf.is_empty() { return Ok(None); }
            return Ok(Some(String::from_utf8_lossy(&buf).into_owned()));
        }
    }
}

pub fn read_req(rd: &mut impl BufRead) -> io::Result<Option<Req>> {
    let Some(line) = read_line_limited(rd, MAX_HEADER_LINE)? else { return Ok(None) };
    let mut it = line.split_whitespace();
    let method = it.next().unwrap_or("").to_ascii_uppercase();
    let path = it.next().unwrap_or("/").to_string();

    let mut key = String::new();
    let mut upgrade = false;
    let mut connection_ok = false;
    let mut version_ok = false;

    for _ in 0..MAX_HEADERS {
        let Some(h) = read_line_limited(rd, MAX_HEADER_LINE)? else { break };
        let h = h.trim_end();
        if h.is_empty() { break; }
        if let Some((k, v)) = h.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim();
            match k.as_str() {
                "upgrade" => upgrade = v.eq_ignore_ascii_case("websocket"),
                "connection" => {
                    connection_ok = v.split(',').any(|t| t.trim().eq_ignore_ascii_case("upgrade"))
                }
                "sec-websocket-version" => version_ok = v == "13",
                "sec-websocket-key" => key = v.to_string(),
                _ => {}
            }
        }
    }

    let ws = method == "GET"
        && upgrade
        && connection_ok
        && version_ok
        && b64_decode(&key).map(|d| d.len() == 16).unwrap_or(false);

    Ok(Some(if ws { Req::Ws(key) } else { Req::Page(path) }))
}

fn head(w: &mut impl Write, status: &str, ctype: &str, len: usize, extra: &str) -> io::Result<()> {
    let s = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {len}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n{extra}\r\n"
    );
    w.write_all(s.as_bytes())
}

pub fn write_html(w: &mut impl Write, body: &str) -> io::Result<()> {
    head(w, "200 OK", "text/html; charset=utf-8", body.len(), "")?;
    w.write_all(body.as_bytes())
}

/// Список комнат: отдаём и по WebSocket, и по HTTP (в т.ч. с другого origin).
pub fn write_json(w: &mut impl Write, body: &str) -> io::Result<()> {
    head(w, "200 OK", "application/json; charset=utf-8", body.len(),
         "Access-Control-Allow-Origin: *\r\n")?;
    w.write_all(body.as_bytes())
}

/// Браузер всегда просит favicon — без этого в логе вечные 404.
pub fn write_icon(w: &mut impl Write) -> io::Result<()> {
    const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'>\
<rect width='32' height='32' rx='7' fill='#0b0f14'/>\
<circle cx='16' cy='16' r='12' fill='none' stroke='#27e0ff' stroke-width='2'/>\
<circle cx='16' cy='16' r='8' fill='none' stroke='#ff3d8b' stroke-width='2'/>\
<circle cx='16' cy='16' r='4' fill='none' stroke='#ffb020' stroke-width='2'/></svg>";
    head(w, "200 OK", "image/svg+xml", SVG.len(), "")?;
    w.write_all(SVG.as_bytes())
}

pub fn write_404(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

pub fn write_101(w: &mut impl Write, key: &str) -> io::Result<()> {
    let mut s = String::from(key);
    s.push_str("258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let accept = b64(&sha1(s.as_bytes()));
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    w.write_all(resp.as_bytes())
}

// ------------------------------------------------------------------ кадры
fn full(rd: &mut impl Read, buf: &mut [u8]) -> io::Result<bool> {
    match rd.read_exact(buf) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e),
    }
}

fn encode(op: u8, p: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(p.len() + 10);
    f.push(0x80 | op);
    let n = p.len();
    if n < 126 { f.push(n as u8); }
    else if n < 65536 { f.push(126); f.extend_from_slice(&(n as u16).to_be_bytes()); }
    else { f.push(127); f.extend_from_slice(&(n as u64).to_be_bytes()); }
    f.extend_from_slice(p);
    f
}

pub fn read_frame(rd: &mut impl Read) -> io::Result<Option<(bool, u8, Vec<u8>)>> {
    let mut h = [0u8; 2];
    if !full(rd, &mut h)? { return Ok(None); }
    let fin = h[0] & 0x80 != 0;
    let rsv = h[0] & 0x70;
    let op = h[0] & 0x0F;
    let masked = h[1] & 0x80 != 0;
    if rsv != 0 { return Err(io::Error::new(io::ErrorKind::InvalidData, "RSV без расширений")); }
    let mut len = (h[1] & 0x7F) as u64;
    if len == 126 { let mut b = [0u8; 2]; if !full(rd, &mut b)? { return Ok(None); } len = u16::from_be_bytes(b) as u64; }
    else if len == 127 { let mut b = [0u8; 8]; if !full(rd, &mut b)? { return Ok(None); } len = u64::from_be_bytes(b); }
    if len > MAX_FRAME as u64 { return Err(io::Error::new(io::ErrorKind::InvalidData, "кадр слишком большой")); }
    if op >= 0x8 && (len > 125 || !fin) { return Err(io::Error::new(io::ErrorKind::InvalidData, "битый control-кадр")); }
    if !masked { return Err(io::Error::new(io::ErrorKind::InvalidData, "клиент обязан маскировать")); }
    let mut mask = [0u8; 4];
    if !full(rd, &mut mask)? { return Ok(None); }
    let mut p = vec![0u8; len as usize];
    if !full(rd, &mut p)? { return Ok(None); }
    for (i, b) in p.iter_mut().enumerate() { *b ^= mask[i & 3]; }
    Ok(Some((fin, op, p)))
}

// ------------------------------------------------------------------ очередь
pub struct Client {
    q: Mutex<Queue>,
    cv: Condvar,
    alive: AtomicBool,
    sock: TcpStream,
}
struct Queue { frames: VecDeque<Vec<u8>>, bytes: usize }

impl Client {
    pub fn new(sock: TcpStream) -> Arc<Self> {
        Arc::new(Self { q: Mutex::new(Queue { frames: VecDeque::new(), bytes: 0 }), cv: Condvar::new(), alive: AtomicBool::new(true), sock })
    }
    pub fn alive(&self) -> bool { self.alive.load(Ordering::Relaxed) }
    pub fn send(&self, msg: &str) { self.push(encode(0x1, msg.as_bytes())); }
    pub fn pong(&self, p: &[u8]) { self.push(encode(0xA, p)); }
    fn push(&self, frame: Vec<u8>) {
        if !self.alive() { return; }
        let n = frame.len();
        let mut q = self.q.lock().unwrap();
        if q.bytes + n > MAX_QUEUE_BYTES { drop(q); self.kill(); return; }
        q.bytes += n;
        q.frames.push_back(frame);
        self.cv.notify_one();
    }
    pub fn kill(&self) {
        if self.alive.swap(false, Ordering::SeqCst) {
            let _ = self.sock.shutdown(Shutdown::Both);
            self.cv.notify_all();
        }
    }
}

pub fn spawn_writer(mut wr: TcpStream, c: Arc<Client>) {
    std::thread::spawn(move || {
        let ping = encode(0x9, b"");
        let mut next_ping = Instant::now() + PING_EVERY;
        loop {
            if !c.alive() { break; }
            let batch = {
                let mut q = c.q.lock().unwrap();
                if q.frames.is_empty() && c.alive() {
                    let wait = next_ping
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(250));
                    let (g, _) = c.cv.wait_timeout(q, wait).unwrap();
                    q = g;
                }
                if !c.alive() { break; }
                let take = std::mem::take(&mut q.frames);
                q.bytes = 0;
                take
            };
            for f in batch {
                if wr.write_all(&f).is_err() { c.kill(); return; }
            }
            if Instant::now() >= next_ping {
                next_ping = Instant::now() + PING_EVERY;
                if wr.write_all(&ping).is_err() { break; }
            }
        }
        let _ = wr.write_all(&encode(0x8, &[]));
        c.kill();
    });
}