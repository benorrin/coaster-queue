use crate::queue::{fetch, Ride, PARKS};
use serde::Serialize;
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_CONNECTIONS: usize = 64;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

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
    ("/fx.js", "text/javascript; charset=utf-8", include_str!("../web/fx.js")),
    ("/font.js", "text/javascript; charset=utf-8", include_str!("../web/font.js")),
];

fn disk_name(path: &str) -> &str {
    match path {
        "/" => "index.html",
        p => p.trim_start_matches('/'),
    }
}

#[derive(Clone, Serialize)]
struct ParkSnapshot {
    id: &'static str,
    name: &'static str,
    rides: Vec<Ride>,
    /// Unix seconds of the last successful fetch.
    fetched_at: Option<u64>,
    error: Option<String>,
}

type Cache = Arc<Mutex<Vec<ParkSnapshot>>>;

pub struct Config {
    pub bind: String,
    pub port: u16,
    pub interval: u64,
    pub web_dir: Option<PathBuf>,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Polls every park on a fixed interval so page loads never hit queue-times directly.
fn spawn_poller(cache: Cache, interval: u64) {
    thread::spawn(move || loop {
        let handles: Vec<_> = PARKS
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let id = p.2;
                (i, thread::spawn(move || fetch(id)))
            })
            .collect();
        for (i, h) in handles {
            let res = h.join().unwrap_or_else(|_| Err("fetch panicked".into()));
            let mut cache = cache.lock().unwrap();
            let snap = &mut cache[i];
            match res {
                Ok(rides) => {
                    snap.rides = rides;
                    snap.fetched_at = Some(now_secs());
                    snap.error = None;
                }
                Err(e) => snap.error = Some(e),
            }
        }
        thread::sleep(Duration::from_secs(interval));
    });
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
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

fn handle(mut stream: TcpStream, cache: &Cache, cfg: &Config) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    // Drain the headers; we don't need any of them.
    let mut seen = request_line.len();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        seen += n;
        if n == 0 || line == "\r\n" || line == "\n" || seen > MAX_HEADER_BYTES {
            break;
        }
    }

    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let head_only = method == "HEAD";
    if method != "GET" && !head_only {
        return respond(&mut stream, "405 Method Not Allowed", "text/plain", b"method not allowed", false);
    }
    let path = target.split(['?', '#']).next().unwrap_or("/");

    match path {
        "/api/queues" => {
            let body = serde_json::to_vec(&*cache.lock().unwrap()).unwrap_or_default();
            respond(&mut stream, "200 OK", "application/json", &body, head_only)
        }
        "/healthz" => respond(&mut stream, "200 OK", "text/plain", b"ok", head_only),
        _ => {
            let Some((_, mime, embedded)) = ASSETS.iter().find(|a| a.0 == path) else {
                return respond(&mut stream, "404 Not Found", "text/plain", b"not found", head_only);
            };
            let body = match &cfg.web_dir {
                Some(dir) => fs::read(dir.join(disk_name(path))).ok(),
                None => Some(embedded.as_bytes().to_vec()),
            };
            match body {
                Some(b) => respond(&mut stream, "200 OK", mime, &b, head_only),
                None => respond(&mut stream, "404 Not Found", "text/plain", b"not found", head_only),
            }
        }
    }
}

pub fn run(cfg: Config) -> io::Result<()> {
    let cache: Cache = Arc::new(Mutex::new(
        PARKS
            .iter()
            .map(|p| ParkSnapshot {
                id: p.0,
                name: p.1,
                rides: Vec::new(),
                fetched_at: None,
                error: None,
            })
            .collect(),
    ));
    spawn_poller(cache.clone(), cfg.interval);

    let listener = TcpListener::bind((cfg.bind.as_str(), cfg.port))?;
    eprintln!("coaster-queue serving on http://{}:{}", cfg.bind, cfg.port);

    let cfg = Arc::new(cfg);
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming().flatten() {
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            let mut s = stream;
            let _ = respond(&mut s, "503 Service Unavailable", "text/plain", b"busy", false);
            continue;
        }
        let (cache, cfg, active) = (cache.clone(), cfg.clone(), active.clone());
        thread::spawn(move || {
            let _ = handle(stream, &cache, &cfg);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}
