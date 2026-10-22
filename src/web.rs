//! HTTP server: the JSON API and, optionally, the web display. Plain std::net with a
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

/// Everything the site serves, embedded so the binary stays self-contained.
/// `--web-dir` reads the same files from disk instead, for front-end work.
const ASSETS: &[(&str, &str, &str)] = &[
    ("/", "text/html; charset=utf-8", include_str!("../web/index.html")),
    ("/style.css", "text/css; charset=utf-8", include_str!("../web/style.css")),
    ("/app.js", "text/javascript; charset=utf-8", include_str!("../web/app.js")),
    ("/led.js", "text/javascript; charset=utf-8", include_str!("../web/led.js")),
    ("/data.js", "text/javascript; charset=utf-8", include_str!("../web/data.js")),
    ("/board.js", "text/javascript; charset=utf-8", include_str!("../web/board.js")),
    ("/controls.js", "text/javascript; charset=utf-8", include_str!("../web/controls.js")),
    ("/settings.js", "text/javascript; charset=utf-8", include_str!("../web/settings.js")),
    ("/weather.js", "text/javascript; charset=utf-8", include_str!("../web/weather.js")),
    ("/fx.js", "text/javascript; charset=utf-8", include_str!("../web/fx.js")),
    ("/font.js", "text/javascript; charset=utf-8", include_str!("../web/font.js")),
];

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

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8], head_only: bool) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if !head_only {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn json(stream: &mut TcpStream, status: &str, value: &impl serde::Serialize, head_only: bool) -> io::Result<()> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    respond(stream, status, "application/json", &body, head_only)
}

fn error(stream: &mut TcpStream, status: &str, message: &str) -> io::Result<()> {
    json(stream, status, &serde_json::json!({ "error": message }), false)
}

fn read_request(stream: &TcpStream) -> io::Result<Option<Request>> {
    let mut reader = BufReader::new(stream.try_clone()?);
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
        let mut h = String::new();
        let n = reader.read_line(&mut h)?;
        seen += n;
        if n == 0 || h == "\r\n" || h == "\n" || seen > MAX_HEADER_BYTES {
            break;
        }
        if let Some((name, value)) = h.split_once(':') {
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

fn list(value: &str) -> Vec<String> {
    value
        .replace("%2C", ",")
        .replace("%2c", ",")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn authorised(ctx: &Ctx, req: &Request) -> bool {
    match &ctx.server.api_token {
        None => true,
        Some(token) => req
            .authorization
            .as_deref()
            .and_then(|a| a.strip_prefix("Bearer "))
            // Compare every byte so the time taken doesn't reveal how much matched.
            .is_some_and(|given| given.len() == token.len() && given.bytes().zip(token.bytes()).fold(0, |d, (a, b)| d | (a ^ b)) == 0),
    }
}

fn api(ctx: &Ctx, stream: &mut TcpStream, req: &Request) -> io::Result<()> {
    let head_only = req.method == "HEAD";
    let writing = !matches!(req.method.as_str(), "GET" | "HEAD");
    let shared = &ctx.shared;

    match req.path.as_str() {
        "/api/queues" if !writing => {
            let settings = shared.settings();
            let slugs = match query_param(&req.query, "parks") {
                Some("all") => shared.store.directory().into_iter().map(|(s, _)| s).collect(),
                Some(v) => list(v),
                None => settings.parks.clone(),
            };
            let show = query_param(&req.query, "show").map_or(settings.show, list);
            let filter = match shared.filter(&show) {
                Ok(f) => f,
                Err(e) => return error(stream, "400 Bad Request", &e),
            };
            shared.store.want(&slugs);
            let views: Vec<_> = slugs.iter().filter_map(|s| shared.store.view(s, &filter)).collect();
            json(stream, "200 OK", &views, head_only)
        }
        "/api/parks" if !writing => {
            let parks: Vec<_> = shared
                .store
                .directory()
                .into_iter()
                .map(|(id, name)| serde_json::json!({ "id": id, "name": name }))
                .collect();
            json(stream, "200 OK", &parks, head_only)
        }
        "/api/groups" if !writing => {
            let groups: Vec<_> = shared.groups(|g| {
                g.list().into_iter().map(|i| serde_json::json!({ "id": i.id, "label": i.label, "builtin": i.builtin })).collect()
            });
            json(stream, "200 OK", &groups, head_only)
        }
        "/api/settings" if !writing => json(stream, "200 OK", &shared.settings(), head_only),
        "/api/settings" if matches!(req.method.as_str(), "PATCH" | "PUT" | "POST") => {
            if !authorised(ctx, req) {
                return error(stream, "401 Unauthorized", "this change needs the API token (Authorization: Bearer ...)");
            }
            let patch: Patch = match serde_json::from_slice(&req.body) {
                Ok(p) => p,
                Err(e) => return error(stream, "400 Bad Request", &e.to_string()),
            };
            match shared.update(patch) {
                Ok(settings) => json(stream, "200 OK", &settings, false),
                Err(e) => error(stream, "400 Bad Request", &e),
            }
        }
        _ => error(stream, "404 Not Found", "no such endpoint"),
    }
}

fn handle(mut stream: TcpStream, ctx: &Ctx) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let Some(req) = read_request(&stream)? else {
        return respond(&mut stream, "400 Bad Request", "text/plain", b"bad request", false);
    };
    let head_only = req.method == "HEAD";
    let can_write = req.path == "/api/settings";
    if !matches!(req.method.as_str(), "GET" | "HEAD") && !can_write {
        return respond(&mut stream, "405 Method Not Allowed", "text/plain", b"method not allowed", false);
    }

    if req.path == "/healthz" {
        return respond(&mut stream, "200 OK", "text/plain", b"ok", head_only);
    }
    if req.path.starts_with("/api/") {
        return if ctx.run.api {
            api(ctx, &mut stream, &req)
        } else {
            respond(&mut stream, "404 Not Found", "text/plain", b"not found", head_only)
        };
    }
    if !ctx.run.web {
        return respond(&mut stream, "404 Not Found", "text/plain", b"not found", head_only);
    }
    let Some((_, mime, embedded)) = ASSETS.iter().find(|a| a.0 == req.path) else {
        return respond(&mut stream, "404 Not Found", "text/plain", b"not found", head_only);
    };
    let body = match &ctx.server.web_dir {
        Some(dir) => fs::read(dir.join(disk_name(&req.path))).ok(),
        None => Some(embedded.as_bytes().to_vec()),
    };
    match body {
        Some(b) => respond(&mut stream, "200 OK", mime, &b, head_only),
        None => respond(&mut stream, "404 Not Found", "text/plain", b"not found", head_only),
    }
}

/// Bind the listening socket, so a bad address or busy port is reported up front.
pub fn bind(server: &Server) -> io::Result<TcpListener> {
    TcpListener::bind((server.bind.as_str(), server.port))
}

/// Serve requests on `listener` until the process exits.
pub fn serve(listener: TcpListener, shared: Arc<Shared>, server: Server, run: Run) {
    let max = server.max_connections.max(1);
    let ctx = Arc::new(Ctx { shared, server, run });
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming().flatten() {
        if active.fetch_add(1, Ordering::SeqCst) >= max {
            active.fetch_sub(1, Ordering::SeqCst);
            let mut s = stream;
            let _ = respond(&mut s, "503 Service Unavailable", "text/plain", b"busy", false);
            continue;
        }
        let (ctx, active2) = (ctx.clone(), active.clone());
        let spawned = thread::Builder::new().stack_size(STACK_BYTES).spawn(move || {
            let _ = handle(stream, &ctx);
            active2.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
