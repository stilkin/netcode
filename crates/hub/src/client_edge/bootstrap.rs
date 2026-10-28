//! The bootstrap endpoint: a minimal hand-rolled HTTP/1.1 responder serving
//! the current certificate hash and a built-in test page, so a browser can
//! pin the hub's per-boot certificate before the lobby API exists
//! (which will absorb this endpoint in a later change).
//!
//! Two routes, nothing else: `GET /cert-hash` (JSON, CORS `*`) and `GET /`
//! (test page). Any other path is 404.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tracing::debug;

use super::Cert;

/// Largest request head we tolerate before giving up (request-line probing).
const MAX_REQUEST_HEAD: usize = 8 * 1024;

/// Serves the bootstrap endpoint until shutdown.
pub async fn serve(listener: TcpListener, cert: Cert, mut shutdown: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else { continue };
                tokio::spawn(serve_one(stream, cert.clone()));
            }
            _ = shutdown.changed() => break,
        }
    }
}

async fn serve_one(mut stream: TcpStream, cert: Cert) {
    let req = match read_request_head(&mut stream).await {
        Some(req) => req,
        None => return,
    };
    let path = req.split_whitespace().nth(1).unwrap_or("");
    let (status, content_type, extra_headers, body) = match path {
        "/cert-hash" => {
            let body = format!("{{\"sha256\":\"{}\"}}", hex(&cert.sha256));
            (
                "200 OK",
                "application/json",
                "Access-Control-Allow-Origin: *\r\n",
                body,
            )
        }
        "/" => (
            "200 OK",
            "text/html; charset=utf-8",
            "",
            TEST_PAGE.to_owned(),
        ),
        _ => (
            "404 Not Found",
            "text/plain; charset=utf-8",
            "",
            String::new(),
        ),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

/// Reads until the end of the request head (`\r\n\r\n`), bounded in size.
async fn read_request_head(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(256);
    let mut chunk = [0u8; 512];
    loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            return Some(String::from_utf8_lossy(&buf).into_owned());
        }
        if buf.len() > MAX_REQUEST_HEAD {
            debug!("bootstrap request head exceeded {MAX_REQUEST_HEAD} bytes");
            return None;
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The built-in single-file test client: fetch the cert hash, connect over
/// WebTransport, join a session, and exercise the echo arena. Requires a
/// browser with WebTransport + `serverCertificateHashes` (Chrome/Edge 122+).
const TEST_PAGE: &str = r#"<!doctype html>
<html lang="en">
<meta charset="utf-8">
<title>netcode hub test client</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  body { font: 14px/1.45 ui-monospace, monospace; margin: 2rem auto; max-width: 46rem; padding: 0 1rem; }
  #log { white-space: pre-wrap; background: #111; color: #9f9; padding: 1rem; border-radius: 6px; min-height: 8rem; }
  input { width: 6rem; }
  label { display: inline-block; width: 5.5rem; }
</style>
<p>netcode hub — echo arena test client</p>
<p>
  <label>WT port</label><input id="wt-port" value="4433">
  <label>session</label><input id="session" value="1">
  <label>name</label><input id="name" value="tester">
  <label>token</label><input id="token" value="">
  <button id="join">join</button>
</p>
<p><button id="send" disabled>send input "hello"</button></p>
<div id="log"></div>
<script type="module">
const log = (line) => { document.getElementById('log').textContent += line + '\n'; };
const le = (buf, off, u32) => new DataView(buf.buffer).setUint32(off, u32, true);

function packet(channel, clientId, sessionId, payload) {
  const buf = new Uint8Array(24 + payload.length);
  const dv = new DataView(buf.buffer);
  buf.set([0x4e,0x48,0x55,0x42], 0);            // "NHUB"
  dv.setUint16(4, 1, true);                     // version
  dv.setUint16(6, 24, true);                    // header size
  buf[8] = channel;                             // channel
  dv.setUint32(10, sessionId, true);
  dv.setUint32(14, clientId, true);
  dv.setUint16(18, 1, true);                    // schema version
  dv.setUint32(20, payload.length, true);
  buf.set(payload, 24);
  return buf;
}

let writer = null;
let dgWriter = null;
let clientId = 0, sessionId = 1;

document.getElementById('join').onclick = async () => {
  try {
    const host = location.hostname;
    const wtPort = document.getElementById('wt-port').value;
    sessionId = +document.getElementById('session').value;
    const { sha256 } = await fetch(`http://${host}:${location.port}/cert-hash`).then(r => r.json());
    log(`cert hash ${sha256.slice(0, 16)}…`);
    const hash = new Uint8Array(32);
    for (let i = 0; i < 64; i += 2) hash[i / 2] = parseInt(sha256.slice(i, i + 2), 16);

    const transport = new WebTransport(`https://${host}:${wtPort}`, {
      serverCertificateHashes: [{ algorithm: 'sha-256', value: hash }],
    });
    await transport.ready;
    dgWriter = transport.datagrams.writable.getWriter();
    log('transport established');

    const stream = await transport.createBidirectionalStream();
    writer = stream.writable.getWriter();
    const reader = stream.readable.getReader();

    // Connect frame
    const enc = new TextEncoder();
    const token = enc.encode(document.getElementById('token').value);
    const name = enc.encode(document.getElementById('name').value);
    const c = new Uint8Array(10 + token.length + name.length);
    const dv = new DataView(c.buffer);
    c[0] = 1; c[1] = 1;
    dv.setUint16(2, token.length, true); c.set(token, 4);
    dv.setUint32(4 + token.length, sessionId, true);
    dv.setUint16(8 + token.length, name.length, true); c.set(name, 10 + token.length);
    await writer.write(c);

    const { value: reply } = await reader.read();
    if (reply[0] === 3) {
      const reason = new TextDecoder().decode(reply.slice(4, 4 + new DataView(reply.buffer).getUint16(2, true)));
      log(`rejected: ${reason}`);
      return;
    }
    clientId = new DataView(reply.buffer).getUint32(2, true);
    log(`joined session ${sessionId} as client ${clientId}`);
    document.getElementById('send').disabled = false;

    // State datagrams → log
    (async () => {
      const dr = transport.datagrams.readable.getReader();
      for (;;) {
        const { value, done } = await dr.read();
        if (done) return;
        const from = new DataView(value.buffer).getUint32(14, true);
        log(`state ← ${new TextDecoder().decode(value.slice(24))} (client ${from})`);
      }
    })();

    // Events on the control stream → log (packets are self-delimiting)
    (async () => {
      let buf = new Uint8Array(0);
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return;
        const merged = new Uint8Array(buf.length + value.length);
        merged.set(buf); merged.set(value, buf.length);
        buf = merged;
        while (buf.length >= 24) {
          const len = new DataView(buf.buffer).getUint32(20, true);
          if (buf.length < 24 + len) break;
          log(`event ← ${new TextDecoder().decode(buf.slice(24, 24 + len))}`);
          buf = buf.slice(24 + len);
        }
      }
    })();

    transport.closed.then(() => log('transport closed'));
  } catch (err) {
    log(`error: ${err}`);
  }
};

document.getElementById('send').onclick = async () => {
  if (!writer || !dgWriter) return;
  const payload = new TextEncoder().encode('hello');
  await dgWriter.write(packet(0, clientId, sessionId, payload));
  log('input → hello');
};
</script>
"#;
