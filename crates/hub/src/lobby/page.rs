//! The built-in test page served at `GET /` — connect by cert hash, browse
//! the lobby API, create a session, and play echo arena. A fixture for
//! manual verification, not a product.

pub const TEST_PAGE: &str = r#"<!doctype html>
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
<p><button id="send" disabled>send input "hello"</button> <button id="refresh">refresh lobby</button></p>
<p>games: <span id="games">(loading…)</span></p>
<p>sessions: <span id="sessions">(loading…)</span></p>
<div id="log"></div>
<script type="module">
const log = (line) => { document.getElementById('log').textContent += line + '\n'; };
const api = location.origin;
const refreshLobby = async () => {
  const games = await fetch(`${api}/api/games`).then(r => r.json()).catch(() => []);
  const gamesEl = document.getElementById('games');
  gamesEl.textContent = '';
  for (const g of games) {
    const btn = document.createElement('button');
    btn.textContent = `create ${g.display_name}`;
    btn.onclick = async () => {
      const resp = await fetch(`${api}/api/sessions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ game_type: g.game_type }),
      });
      const body = await resp.json().catch(() => ({}));
      if (resp.status === 201) {
        log(`created session ${body.session_id} (${g.game_type})`);
        document.getElementById('session').value = body.session_id;
        refreshLobby();
      } else {
        log(`create failed: ${body.error ?? resp.status}`);
      }
    };
    gamesEl.append(btn, ' ');
  }
  if (!games.length) gamesEl.textContent = '(no [[games]] configured)';

  const sessions = await fetch(`${api}/api/sessions`).then(r => r.json()).catch(() => []);
  const sessionsEl = document.getElementById('sessions');
  sessionsEl.textContent = sessions.length
    ? sessions.map((s) => `#${s.id} ${s.game_type} [${s.state}] ${s.player_count}/${s.max_players}`).join(' · ')
    : '(none)';
};
document.getElementById('refresh').onclick = refreshLobby;
refreshLobby();
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
