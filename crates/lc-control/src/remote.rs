//! Fjärrkontroll i mobilen: en liten webbserver i det lokala nätverket som
//! visar en sida med GO, Svart, master och cue-listan. Ingen app behövs –
//! mobilen öppnar sidan via en QR-kod i LuxCanvas.
//!
//! Adressen innehåller en hemlig nyckel (`?k=…`). Utan den svarar servern
//! bara "403", så att ingen annan i nätverket kan styra showen.

use lc_core::i18n::t;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Det mobilen ber om.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteCmd {
    Go,
    Prev,
    /// Gå till cue nummer (0-baserat).
    Cue(usize),
    Blackout(bool),
    Master(f32),
    PlayPause,
}

pub struct RemoteServer {
    rx: Receiver<RemoteCmd>,
    state: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    port: u16,
}

impl RemoteServer {
    /// Startar servern på `port` (alla nätverkskort). `token` är nyckeln som
    /// måste finnas i adressen.
    pub fn start(port: u16, token: String) -> Result<Self, String> {
        let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("{} {port}: {e}", t("Kunde inte öppna port", "Could not open port")))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new("{}".to_string()));
        let stop = Arc::new(AtomicBool::new(false));
        let page = page(&token);
        let (st, sp) = (state.clone(), stop.clone());
        std::thread::Builder::new()
            .name("luxcanvas-remote".into())
            .spawn(move || {
                while !sp.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if let Err(e) = serve(stream, &token, &page, &st, &tx) {
                                log::debug!("remote: {e}");
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(30)),
                        Err(e) => {
                            log::warn!("remote: {e}");
                            std::thread::sleep(Duration::from_millis(200));
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(RemoteServer { rx, state, stop, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Kommandon som kommit sedan förra anropet.
    pub fn poll(&self) -> Vec<RemoteCmd> {
        self.rx.try_iter().collect()
    }

    /// Läget som mobilen visar, som JSON (se `state_json`).
    pub fn set_state(&self, json: String) {
        if let Ok(mut s) = self.state.lock() {
            *s = json;
        }
    }
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// En cue som mobilen visar.
pub struct CueInfo<'a> {
    pub name: &'a str,
    pub fade: f32,
    pub auto: Option<f32>,
}

/// Läget som JSON för mobilsidan.
pub fn state_json(project: &str, cues: &[CueInfo], current: Option<usize>, next: Option<usize>, blackout: bool, master: f32, playing: bool) -> String {
    let cues: Vec<String> = cues
        .iter()
        .map(|c| format!("{{\"name\":{},\"fade\":{},\"auto\":{}}}", json_string(c.name), c.fade, c.auto.map_or("null".into(), |a| a.to_string())))
        .collect();
    let idx = |i: Option<usize>| i.map_or("null".to_string(), |i| i.to_string());
    format!(
        "{{\"project\":{},\"cues\":[{}],\"current\":{},\"next\":{},\"black\":{blackout},\"master\":{master},\"playing\":{playing}}}",
        json_string(project),
        cues.join(","),
        idx(current),
        idx(next)
    )
}

/// En JSON-sträng med citattecken.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            // Skyddar mot att text i projektet avslutar <script> om JSON bäddas in i HTML.
            '<' => out.push_str("\\u003c"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Datorns adress i det lokala nätverket, t.ex. 192.168.1.20. (Inget skickas:
/// en UDP-socket "kopplas" bara för att se vilket nätverkskort som används.)
pub fn local_ip() -> Option<std::net::IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:80").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

/// En ny slumpad nyckel, 16 hextecken.
pub fn new_token() -> String {
    let mut bytes = [0u8; 8];
    let ok = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).is_ok();
    if !ok {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        bytes = (now.as_nanos() as u64 ^ std::process::id() as u64).to_le_bytes();
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Delar upp "/väg?a=1&b=2".
fn split_target(target: &str) -> (&str, Vec<(&str, &str)>) {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let params = query.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').unwrap_or((p, ""))).collect();
    (path, params)
}

/// Tolkar en förfrågan till ett kommando. `None` för okänd väg.
pub fn parse_command(path: &str, params: &[(&str, &str)]) -> Option<RemoteCmd> {
    let get = |k: &str| params.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
    Some(match path {
        "/api/go" => RemoteCmd::Go,
        "/api/prev" => RemoteCmd::Prev,
        "/api/play" => RemoteCmd::PlayPause,
        "/api/cue" => RemoteCmd::Cue(get("i")?.parse().ok()?),
        "/api/black" => RemoteCmd::Blackout(get("on")? == "1"),
        "/api/master" => RemoteCmd::Master(get("v")?.parse::<f32>().ok()?.clamp(0.0, 1.0)),
        _ => return None,
    })
}

fn serve(mut stream: TcpStream, token: &str, page: &str, state: &Mutex<String>, tx: &Sender<RemoteCmd>) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut buf = [0u8; 4096];
    let mut len = 0;
    while len < buf.len() {
        let n = stream.read(&mut buf[len..])?;
        if n == 0 {
            break;
        }
        len += n;
        if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let request = String::from_utf8_lossy(&buf[..len]);
    let target = request.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/");
    let (path, params) = split_target(target);
    let authorised = params.iter().any(|(k, v)| *k == "k" && *v == token);

    let (status, kind, body): (&str, &str, String) = if !authorised {
        let text = t("Skanna QR-koden i LuxCanvas igen.", "Scan the QR code in LuxCanvas again.");
        ("403 Forbidden", "text/plain; charset=utf-8", text.to_string())
    } else if path == "/" {
        ("200 OK", "text/html; charset=utf-8", page.to_string())
    } else if path == "/api/state" {
        ("200 OK", "application/json", state.lock().map(|s| s.clone()).unwrap_or_default())
    } else if let Some(cmd) = parse_command(path, &params) {
        let _ = tx.send(cmd);
        ("200 OK", "text/plain", "ok".into())
    } else {
        ("404 Not Found", "text/plain", "404".into())
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

/// Mobilsidan. Texterna följer LuxCanvas språk.
fn page(token: &str) -> String {
    PAGE.replace("{TOKEN}", token)
        .replace("{LANG}", if t("sv", "en") == "sv" { "sv" } else { "en" })
        .replace("{NEXT}", t("Nästa", "Next"))
        .replace("{LAST}", t("Sista cuen", "Last cue"))
        .replace("{BLACK}", t("Svart", "Black"))
        .replace("{PREV}", t("◀ Förra", "◀ Back"))
        .replace("{PLAY}", t("⏯ Spela", "⏯ Play"))
        .replace("{MASTER}", t("Ljusstyrka", "Brightness"))
        .replace("{OFFLINE}", t("Ingen kontakt med LuxCanvas…", "No contact with LuxCanvas…"))
        .replace("{NOCUES}", t("Inga cues i projektet. GO och Svart fungerar ändå.", "No cues in the project. GO and Black still work."))
}

const PAGE: &str = r##"<!doctype html>
<html lang="{LANG}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="theme-color" content="#111418">
<title>LuxCanvas</title>
<style>
  :root { color-scheme: dark; --bg:#111418; --panel:#1c2128; --text:#e8edf2; --dim:#8b96a3; --accent:#00beff; --go:#2e8b45; --black:#a33; }
  * { box-sizing: border-box; -webkit-tap-highlight-color: transparent; }
  body { margin: 0; background: var(--bg); color: var(--text); font: 16px/1.35 system-ui, sans-serif; padding: 16px; padding-bottom: 40px; }
  header { display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 12px; }
  h1 { font-size: 20px; margin: 0; color: var(--accent); }
  #project { color: var(--dim); font-size: 14px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 60%; }
  #go { width: 100%; height: 30vh; min-height: 150px; max-height: 260px; border: 0; border-radius: 18px; background: var(--go); color: #fff; font-size: 48px; font-weight: 700; }
  #go small { display: block; font-size: 16px; font-weight: 400; opacity: .85; margin-top: 6px; }
  #go:active { filter: brightness(1.25); }
  .row { display: flex; gap: 10px; margin-top: 10px; }
  .row button { flex: 1; height: 56px; border: 0; border-radius: 12px; background: var(--panel); color: var(--text); font-size: 17px; }
  #black.on { background: var(--black); color: #fff; }
  label { display: block; margin: 18px 0 6px; color: var(--dim); font-size: 14px; }
  input[type=range] { width: 100%; height: 36px; accent-color: var(--accent); }
  ol { list-style: none; padding: 0; margin: 18px 0 0; }
  li button { width: 100%; text-align: left; display: flex; justify-content: space-between; gap: 8px; padding: 14px 14px; margin-bottom: 6px; border: 0; border-radius: 10px; background: var(--panel); color: var(--text); font-size: 17px; }
  li button .meta { color: var(--dim); font-size: 14px; white-space: nowrap; }
  li.current button { background: #1f5a30; }
  li.next button { outline: 2px solid #ffc850; }
  #status { color: #ffb070; text-align: center; margin-top: 14px; min-height: 1.4em; }
  .empty { color: var(--dim); }
</style>
</head>
<body>
<header><h1>LuxCanvas</h1><span id="project"></span></header>
<button id="go">GO ▶<small id="nextname"></small></button>
<div class="row">
  <button id="prev">{PREV}</button>
  <button id="black">⏹ {BLACK}</button>
  <button id="play">{PLAY}</button>
</div>
<label for="master">{MASTER}</label>
<input id="master" type="range" min="0" max="1" step="0.01" value="1">
<ol id="cues"></ol>
<div id="status"></div>
<script>
const K = "{TOKEN}";
let state = null, dragging = false;
const $ = id => document.getElementById(id);
function send(path) { return fetch(path + (path.includes("?") ? "&" : "?") + "k=" + K, { method: "POST" }).then(refresh).catch(() => {}); }
function render() {
  if (!state) return;
  $("project").textContent = state.project;
  $("black").classList.toggle("on", state.black);
  if (!dragging) $("master").value = state.master;
  const next = state.next === null ? null : state.cues[state.next];
  $("nextname").textContent = next ? "{NEXT}: " + next.name : (state.cues.length ? "{LAST}" : "");
  const list = $("cues");
  list.innerHTML = "";
  if (!state.cues.length) { const p = document.createElement("li"); p.className = "empty"; p.textContent = "{NOCUES}"; list.appendChild(p); }
  state.cues.forEach((c, i) => {
    const li = document.createElement("li");
    if (i === state.current) li.className = "current"; else if (i === state.next) li.className = "next";
    const b = document.createElement("button");
    const name = document.createElement("span"); name.textContent = (i === state.current ? "▶ " : "") + (i + 1) + ". " + c.name;
    const meta = document.createElement("span"); meta.className = "meta"; meta.textContent = (c.auto ? "⏱ " + c.auto + " s · " : "") + c.fade + " s";
    b.append(name, meta);
    b.onclick = () => send("/api/cue?i=" + i);
    li.appendChild(b); list.appendChild(li);
  });
}
function refresh() {
  return fetch("/api/state?k=" + K, { cache: "no-store" })
    .then(r => r.json()).then(s => { state = s; $("status").textContent = ""; render(); })
    .catch(() => { $("status").textContent = "{OFFLINE}"; });
}
$("go").onclick = () => send("/api/go");
$("prev").onclick = () => send("/api/prev");
$("play").onclick = () => send("/api/play");
$("black").onclick = () => send("/api/black?on=" + (state && state.black ? 0 : 1));
const m = $("master");
m.oninput = () => { dragging = true; fetch("/api/master?v=" + m.value + "&k=" + K, { method: "POST" }).catch(() => {}); };
m.onchange = () => { dragging = false; };
refresh(); setInterval(refresh, 800);
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_parse() {
        let p = |t: &str| {
            let (path, params) = split_target(t);
            parse_command(path, &params)
        };
        assert_eq!(p("/api/go?k=x"), Some(RemoteCmd::Go));
        assert_eq!(p("/api/cue?i=3&k=x"), Some(RemoteCmd::Cue(3)));
        assert_eq!(p("/api/black?on=1"), Some(RemoteCmd::Blackout(true)));
        assert_eq!(p("/api/master?v=2.5"), Some(RemoteCmd::Master(1.0)));
        assert_eq!(p("/api/cue?i=x"), None);
        assert_eq!(p("/nope"), None);
    }

    #[test]
    fn json_is_escaped() {
        assert_eq!(json_string("a\"b</script>"), "\"a\\\"b\\u003c/script>\"");
        let cues = [CueInfo { name: "Natt", fade: 2.0, auto: Some(10.0) }];
        let s = state_json("x", &cues, Some(0), None, true, 0.5, false);
        assert!(s.contains("\"current\":0") && s.contains("\"next\":null") && s.contains("\"auto\":10"));
    }

    #[test]
    fn server_needs_the_token() {
        let port = 18077;
        let Ok(server) = RemoteServer::start(port, "abc".into()) else { return };
        server.set_state("{\"ok\":1}".into());
        let get = |path: &str| {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        assert!(get("/?k=wrong").starts_with("HTTP/1.1 403"));
        assert!(get("/api/state?k=abc").ends_with("{\"ok\":1}"));
        assert!(get("/api/go?k=abc").starts_with("HTTP/1.1 200"));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(server.poll(), vec![RemoteCmd::Go]);
    }
}
