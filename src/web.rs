//! HTTP server: the JSON API and, optionally, the web display. Plain `std::net` with a
//! small thread per connection (small stack, capped count) to stay light on a Pi.

use crate::config::{Run, Server};
use crate::settings::Patch;
use crate::store::Shared;
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const STACK_BYTES: usize = 256 * 1024;

const HTML: &str = "text/html; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";
const TEXT: &str = "text/plain";

/// One embedded file of the web display, served at "/" + its name.
macro_rules! asset {
    ($file:literal, $mime:expr) => {
        (concat!("/", $file), $mime, include_str!(concat!("../web/", $file)))
    };
}

/// Everything the web display serves, embedded so the binary stays self-contained
/// (`--web-dir` reads the same files from disk instead, for front-end work). A test
/// checks that every file in `web/` is listed here.
const ASSETS: &[(&str, &str, &str)] = &[
    ("/", HTML, include_str!("../web/index.html")),
    asset!("style.css", CSS),
    asset!("app.js", JS),
    asset!("board.js", JS),
    asset!("color.js", JS),
    asset!("controls.js", JS),
    asset!("data.js", JS),
    asset!("demo.js", JS),
    asset!("font.js", JS),
    asset!("fx.js", JS),
    asset!("layout.js", JS),
    asset!("led.js", JS),
    asset!("overlays.js", JS),
    asset!("settings.js", JS),
    asset!("util.js", JS),
    asset!("weather.js", JS),
];

/// The file on disk behind a served path, for `--web-dir`.
fn disk_name(path: &str) -> &str {
    match path {
        "/" => "index.html",
        p => p.trim_start_matches('/'),
    }
}

struct Ctx {
    shared: Arc<Shared>,
    server: Server,
    run: Run,
}

struct Request {
    method: String,
    path: String,
    query: String,
    authorization: Option<String>,
    body: Vec<u8>,
}

impl Request {
    /// GET and HEAD only read; everything else changes something.
    fn is_read(&self) -> bool {
        matches!(self.method.as_str(), "GET" | "HEAD")
    }
}

/// Writes one response. HEAD requests get the headers but no body.
struct Reply<'a> {
    stream: &'a mut TcpStream,
    head_only: bool,
}

impl Reply<'_> {
    fn send(&mut self, status: &str, content_type: &str, body: &[u8]) -> io::Result<()> {
        write!(
            self.stream,
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
             Cache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        if !self.head_only {
            self.stream.write_all(body)?;
        }
        self.stream.flush()
    }

    fn json(&mut self, status: &str, value: &impl serde::Serialize) -> io::Result<()> {
        let body = serde_json::to_vec(value).unwrap_or_default();
        self.send(status, "application/json", &body)
    }

    fn error(&mut self, status: &str, message: &str) -> io::Result<()> {
        self.json(status, &serde_json::json!({ "error": message }))
    }

    fn not_found(&mut self) -> io::Result<()> {
        self.send("404 Not Found", TEXT, b"not found")
    }
}

/// Read the request line, the headers we use and any body. None means it was malformed
/// or too large.
fn read_request(stream: &TcpStream) -> io::Result<Option<Request>> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else { return Ok(None) };
    let target = target.split('#').next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut req = Request {
        method: method.to_string(),
        path: path.to_string(),
        query: query.to_string(),
        authorization: None,
        body: Vec::new(),
    };

    let (mut seen, mut length) = (line.len(), 0usize);
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        seen += n;
        if n == 0 || header.trim().is_empty() || seen > MAX_HEADER_BYTES {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().unwrap_or(0),
                "authorization" => req.authorization = Some(value.trim().to_string()),
                _ => {}
            }
        }
    }
    if length > MAX_BODY_BYTES {
        return Ok(None);
    }
    req.body = vec![0; length];
    reader.read_exact(&mut req.body)?;
    Ok(Some(req))
}

/// The value of `name` in a query string like `a=1&b=x,y`.
fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
}

/// Split a comma separated query value (browsers may send the commas as %2C).
fn list(value: &str) -> Vec<String> {
    let value = value.replace("%2C", ",").replace("%2c", ",");
    value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// Whether the request may change settings: always, unless an API token is configured.
fn authorised(ctx: &Ctx, req: &Request) -> bool {
    let Some(token) = &ctx.server.api_token else { return true };
    let given = req.authorization.as_deref().and_then(|a| a.strip_prefix("Bearer "));
    // Compare every byte so the time taken doesn't reveal how much of the token matched.
    given.is_some_and(|g| {
        g.len() == token.len() && g.bytes().zip(token.bytes()).fold(0, |d, (a, b)| d | (a ^ b)) == 0
    })
}

/// `/api/queues?parks=a,b&show=x,y`: queue times for the chosen parks, falling back to the
/// configured parks and groups for whichever parameter is missing.
fn queues(ctx: &Ctx, reply: &mut Reply, query: &str) -> io::Result<()> {
    let shared = &ctx.shared;
    let settings = shared.settings();
    let slugs = match query_param(query, "parks") {
        Some("all") => shared.store.directory().into_iter().map(|(slug, _)| slug).collect(),
        Some(value) => list(value),
        None => settings.parks,
    };
    let show = query_param(query, "show").map_or(settings.show, list);
    let filter = match shared.filter(&show) {
        Ok(f) => f,
        Err(e) => return reply.error("400 Bad Request", &e),
    };
    shared.store.want(&slugs);
    let views: Vec<_> = slugs.iter().filter_map(|slug| shared.store.view(slug, &filter)).collect();
    reply.json("200 OK", &views)
}

/// `PATCH /api/settings` with a JSON object of the settings to change.
fn change_settings(ctx: &Ctx, reply: &mut Reply, req: &Request) -> io::Result<()> {
    if !authorised(ctx, req) {
        return reply
            .error("401 Unauthorized", "this change needs the API token (Authorization: Bearer ...)");
    }
    let patch: Patch = match serde_json::from_slice(&req.body) {
        Ok(p) => p,
        Err(e) => return reply.error("400 Bad Request", &e.to_string()),
    };
    match ctx.shared.update(patch) {
        Ok(settings) => reply.json("200 OK", &settings),
        Err(e) => reply.error("400 Bad Request", &e),
    }
}

fn api(ctx: &Ctx, reply: &mut Reply, req: &Request) -> io::Result<()> {
    let shared = &ctx.shared;
    match (req.is_read(), req.path.as_str()) {
        (true, "/api/queues") => queues(ctx, reply, &req.query),
        (true, "/api/parks") => {
            let parks: Vec<_> = shared.store.directory();
            let parks: Vec<_> =
                parks.iter().map(|(id, name)| serde_json::json!({ "id": id, "name": name })).collect();
            reply.json("200 OK", &parks)
        }
        (true, "/api/groups") => {
            let groups: Vec<_> = shared.groups(|g| {
                g.list()
                    .into_iter()
                    .map(|i| serde_json::json!({ "id": i.id, "label": i.label, "builtin": i.builtin }))
                    .collect()
            });
            reply.json("200 OK", &groups)
        }
        (true, "/api/settings") => reply.json("200 OK", &shared.settings()),
        (false, "/api/settings") if matches!(req.method.as_str(), "PATCH" | "PUT" | "POST") => {
            change_settings(ctx, reply, req)
        }
        (false, "/api/settings") => reply.send("405 Method Not Allowed", TEXT, b"method not allowed"),
        _ => reply.not_found(),
    }
}

/// Serve one file of the web display.
fn asset(ctx: &Ctx, reply: &mut Reply, path: &str) -> io::Result<()> {
    let Some(&(_, mime, embedded)) = ASSETS.iter().find(|a| a.0 == path) else { return reply.not_found() };
    match &ctx.server.web_dir {
        Some(dir) => match fs::read(dir.join(disk_name(path))) {
            Ok(body) => reply.send("200 OK", mime, &body),
            Err(_) => reply.not_found(),
        },
        None => reply.send("200 OK", mime, embedded.as_bytes()),
    }
}

fn handle(mut stream: TcpStream, ctx: &Ctx) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let Some(req) = read_request(&stream)? else {
        return Reply { stream: &mut stream, head_only: false }.send("400 Bad Request", TEXT, b"bad request");
    };
    let mut reply = Reply { stream: &mut stream, head_only: req.method == "HEAD" };

    match req.path.as_str() {
        _ if !req.is_read() && req.path != "/api/settings" => {
            reply.send("405 Method Not Allowed", TEXT, b"method not allowed")
        }
        "/healthz" => reply.send("200 OK", TEXT, b"ok"),
        path if path.starts_with("/api/") && ctx.run.api => api(ctx, &mut reply, &req),
        path if ctx.run.web => asset(ctx, &mut reply, path),
        _ => reply.not_found(),
    }
}

/// Bind the listening socket, so a bad address or busy port is reported up front.
pub fn bind(server: &Server) -> io::Result<TcpListener> {
    TcpListener::bind((server.bind.as_str(), server.port))
}

/// Serve requests on `listener` until the process exits. Each connection gets its own
/// small thread, up to `max_connections`; beyond that clients are told to retry.
pub fn serve(listener: &TcpListener, shared: Arc<Shared>, server: Server, run: Run) {
    let max = server.max_connections.max(1);
    let ctx = Arc::new(Ctx { shared, server, run });
    let active = Arc::new(AtomicUsize::new(0));
    for mut stream in listener.incoming().flatten() {
        if active.fetch_add(1, Ordering::SeqCst) >= max {
            active.fetch_sub(1, Ordering::SeqCst);
            let _ = Reply { stream: &mut stream, head_only: false }.send(
                "503 Service Unavailable",
                TEXT,
                b"busy",
            );
            continue;
        }
        let (ctx, active_in_thread) = (ctx.clone(), active.clone());
        let spawned = thread::Builder::new().stack_size(STACK_BYTES).spawn(move || {
            let _ = handle(stream, &ctx);
            active_in_thread.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file in `web/` must be in `ASSETS`, or the page would silently break.
    #[test]
    fn every_web_file_is_served() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/web");
        for entry in fs::read_dir(dir).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            let path = if name == "index.html" { "/".to_string() } else { format!("/{name}") };
            assert!(ASSETS.iter().any(|a| a.0 == path), "{name} is not listed in ASSETS");
        }
    }

    #[test]
    fn query_parsing() {
        assert_eq!(query_param("parks=a,b&show=x", "show"), Some("x"));
        assert_eq!(query_param("parks=a", "show"), None);
        assert_eq!(query_param("show=", "show"), Some(""));
        assert_eq!(list("a%2Cb, c,,"), ["a", "b", "c"]);
    }
}
