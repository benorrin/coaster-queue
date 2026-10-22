//! The HTTP server end to end: a real server on an ephemeral port, driven over a raw TCP
//! socket. The poller is never started, so nothing here touches the network.

use coaster_queue::config::{Run, Server};
use coaster_queue::groups::Groups;
use coaster_queue::settings::Settings;
use coaster_queue::store::Shared;
use coaster_queue::web;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    thread,
    time::Duration,
};

const API: Run = Run { led: false, api: true, web: false };
const API_AND_WEB: Run = Run { led: false, api: true, web: true };

/// A running server, and the address to reach it at.
struct TestServer {
    addr: SocketAddr,
}

impl TestServer {
    fn start(run: Run) -> TestServer {
        TestServer::start_with(run, Server { port: 0, ..Server::default() })
    }

    fn start_with(run: Run, server: Server) -> TestServer {
        let shared = Shared::new(Settings::default(), Groups::builtin());
        let listener = web::bind(&server).expect("bind an ephemeral port");
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || web::serve(&listener, shared, server, run));
        TestServer { addr }
    }

    /// Send one request and read the whole response.
    fn request(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Response {
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n");
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        self.raw(head.as_bytes())
    }

    fn raw(&self, bytes: &[u8]) -> Response {
        let mut stream = TcpStream::connect(self.addr).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        stream.write_all(bytes).unwrap();
        read_response(stream)
    }

    fn get(&self, path: &str) -> Response {
        self.request("GET", path, &[], "")
    }

    fn patch(&self, body: &str) -> Response {
        self.request("PATCH", "/api/settings", &[], body)
    }
}

struct Response {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

impl Response {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", self.text()))
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.lines().find_map(|l| {
            let (n, v) = l.split_once(':')?;
            n.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }
}

fn read_response(mut stream: TcpStream) -> Response {
    let mut raw = Vec::new();
    // The server closes the connection after replying; a reset after that is fine.
    let _ = stream.read_to_end(&mut raw);
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("a complete response head");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let (status_line, headers) = head.split_once("\r\n").unwrap_or((&head, ""));
    let status = status_line.split_whitespace().nth(1).unwrap().parse().unwrap();
    Response { status, headers: headers.to_string(), body: raw[split + 4..].to_vec() }
}

fn error_of(r: &Response) -> String {
    r.json()["error"].as_str().unwrap_or_else(|| panic!("no error in {:?}", r.text())).to_string()
}

// ---- reading ----------------------------------------------------------------

#[test]
fn healthz() {
    let s = TestServer::start(API);
    let r = s.get("/healthz");
    assert_eq!((r.status, r.text().as_str()), (200, "ok"));
}

#[test]
fn settings_default() {
    let s = TestServer::start(API);
    let r = s.get("/api/settings");
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("application/json"));
    let v = r.json();
    assert_eq!(v["parks"], json!(["alton-towers", "thorpe-park"]));
    assert_eq!((v["dwell"].clone(), v["theme"].clone()), (json!(8), json!("rgb")));
}

#[test]
fn parks_and_groups_are_listed() {
    let s = TestServer::start(API);
    let parks = s.get("/api/parks").json();
    assert!(parks.as_array().unwrap().contains(&json!({"id": "thorpe-park", "name": "Thorpe Park"})));
    let groups = s.get("/api/groups").json();
    assert!(groups.as_array().unwrap().iter().any(|g| g["id"] == "thrill" && g["builtin"] == true));
}

#[test]
fn queues_for_configured_parks_before_any_fetch() {
    let s = TestServer::start(API);
    let v = s.get("/api/queues").json();
    let ids: Vec<&str> = v.as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["alton-towers", "thorpe-park"]);
    assert_eq!(v[0]["rides"], json!([]));
    assert_eq!(v[0]["fetched_at"], Value::Null);
}

#[test]
fn queues_parks_param_picks_parks_and_skips_unknown_ones() {
    let s = TestServer::start(API);
    let v = s.get("/api/queues?parks=thorpe-park,nowhere").json();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["id"], "thorpe-park");
    // Browsers may escape the comma.
    assert_eq!(s.get("/api/queues?parks=thorpe-park%2Calton-towers").json().as_array().unwrap().len(), 2);
}

#[test]
fn queues_with_an_unknown_group_is_a_400() {
    let s = TestServer::start(API);
    let r = s.get("/api/queues?show=nope");
    assert_eq!(r.status, 400);
    assert!(error_of(&r).contains("unknown group"));
    assert_eq!(s.get("/api/queues?show=thrill").status, 200);
    assert_eq!(s.get("/api/queues?show=").status, 200, "empty show means every ride");
}

#[test]
fn unknown_api_path_is_a_404() {
    let s = TestServer::start(API);
    assert_eq!(s.get("/api/nope").status, 404);
}

#[test]
fn head_has_headers_but_no_body() {
    let s = TestServer::start(API);
    let r = s.request("HEAD", "/healthz", &[], "");
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-length"), Some("2"));
    assert!(r.body.is_empty());
}

// ---- changing settings ------------------------------------------------------

#[test]
fn patch_changes_settings_and_get_sees_them() {
    let s = TestServer::start(API);
    let r = s.patch(r#"{"dwell": 12, "theme": "amber", "parks": ["thorpe-park"], "show": ["thrill"]}"#);
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["dwell"], 12, "the reply is the new settings");
    let now = s.get("/api/settings").json();
    assert_eq!((now["dwell"].clone(), now["theme"].clone()), (json!(12), json!("amber")));
    assert_eq!(now["parks"], json!(["thorpe-park"]));
    assert_eq!(now["show"], json!(["thrill"]));
    // The configured parks now drive /api/queues too.
    assert_eq!(s.get("/api/queues").json().as_array().unwrap().len(), 1);
}

#[test]
fn put_and_post_also_change_settings() {
    let s = TestServer::start(API);
    for method in ["PUT", "POST"] {
        assert_eq!(s.request(method, "/api/settings", &[], r#"{"dwell": 20}"#).status, 200, "{method}");
    }
}

/// Regression: a patch with one bad value must change nothing, not apply its good parts.
#[test]
fn a_rejected_patch_changes_nothing() {
    let s = TestServer::start(API);
    let before = s.get("/api/settings").json();
    let r = s.patch(r#"{"dwell": 30, "theme": "neon"}"#);
    assert_eq!(r.status, 400);
    assert!(error_of(&r).contains("theme"));
    assert_eq!(s.get("/api/settings").json(), before);
}

#[test]
fn bad_patches_are_400s_with_a_message() {
    let s = TestServer::start(API);
    for (body, mentions) in [
        (r#"{"dwel": 5}"#, "unknown field"),
        (r#"{"dwell": "soon"}"#, "invalid type"),
        (r#"{"dwell": 1}"#, "dwell"),
        (r#"{"brightness": 5}"#, "brightness"),
        (r#"{"parks": []}"#, "at least one"),
        (r#"{"parks": ["not-a-park"]}"#, "unknown park"),
        (r#"{"parks": ["Has Spaces"]}"#, "valid name"),
        (r#"{"show": ["not-a-group"]}"#, "unknown group"),
        ("not json", "expected"),
        ("", "EOF"),
    ] {
        let r = s.patch(body);
        assert_eq!(r.status, 400, "{body}");
        assert!(error_of(&r).contains(mentions), "{body}: {}", error_of(&r));
    }
}

#[test]
fn other_methods_on_settings_are_405s() {
    let s = TestServer::start(API);
    for method in ["DELETE", "OPTIONS"] {
        assert_eq!(s.request(method, "/api/settings", &[], "").status, 405, "{method}");
    }
}

/// Regression: only /api/settings accepts writes; everything else rejects them before routing.
#[test]
fn writes_elsewhere_are_405s() {
    let s = TestServer::start(API_AND_WEB);
    for path in ["/api/queues", "/healthz", "/"] {
        assert_eq!(s.request("POST", path, &[], "{}").status, 405, "{path}");
    }
}

// ---- the API token ----------------------------------------------------------

fn with_token() -> TestServer {
    TestServer::start_with(API, Server { port: 0, api_token: Some("s3cret".into()), ..Server::default() })
}

#[test]
fn changes_need_the_token_when_one_is_set() {
    let s = with_token();
    let body = r#"{"dwell": 15}"#;
    let call = |auth: Option<&str>| {
        let headers: Vec<(&str, &str)> = auth.map(|a| ("Authorization", a)).into_iter().collect();
        s.request("PATCH", "/api/settings", &headers, body).status
    };
    assert_eq!(call(None), 401);
    assert_eq!(call(Some("Bearer wrong!")), 401, "same length, wrong bytes");
    assert_eq!(call(Some("Bearer s3cre")), 401, "a prefix of the token");
    assert_eq!(call(Some("Bearer s3cret-and-more")), 401, "longer than the token");
    assert_eq!(call(Some("Bearer")), 401);
    assert_eq!(call(Some("s3cret")), 401, "needs the Bearer scheme");
    assert_eq!(call(Some("Basic s3cret")), 401);
    assert_eq!(s.get("/api/settings").json()["dwell"], 8, "nothing changed so far");
    assert_eq!(call(Some("Bearer s3cret")), 200);
    assert_eq!(s.get("/api/settings").json()["dwell"], 15);
}

#[test]
fn reads_stay_open_when_a_token_is_set() {
    let s = with_token();
    for path in ["/api/settings", "/api/queues", "/api/parks", "/api/groups", "/healthz"] {
        assert_eq!(s.get(path).status, 200, "{path}");
    }
}

#[test]
fn the_401_explains_itself() {
    let r = with_token().patch("{}");
    assert!(error_of(&r).contains("Bearer"));
}

// ---- the web display --------------------------------------------------------

#[test]
fn web_display_is_served_when_enabled() {
    let s = TestServer::start(API_AND_WEB);
    let index = s.get("/");
    assert_eq!(index.status, 200);
    assert_eq!(index.header("content-type"), Some("text/html; charset=utf-8"));
    assert!(index.text().contains("<html"));
    let js = s.get("/app.js");
    assert_eq!((js.status, js.header("content-type")), (200, Some("text/javascript; charset=utf-8")));
    assert_eq!(s.get("/style.css").header("content-type"), Some("text/css; charset=utf-8"));
    assert_eq!(s.get("/nope.js").status, 404);
}

#[test]
fn every_file_in_web_is_served_with_its_contents() {
    let s = TestServer::start(API_AND_WEB);
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web");
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let path = if name == "index.html" { "/".to_string() } else { format!("/{name}") };
        let r = s.get(&path);
        assert_eq!(r.status, 200, "{path}");
        assert_eq!(r.body, std::fs::read(entry.path()).unwrap(), "{path} differs from web/{name}");
    }
}

#[test]
fn web_display_is_not_served_when_disabled() {
    let s = TestServer::start(API);
    assert_eq!(s.get("/").status, 404);
    assert_eq!(s.get("/app.js").status, 404);
}

#[test]
fn api_is_not_served_when_disabled() {
    let s = TestServer::start(Run { led: false, api: false, web: true });
    assert_eq!(s.get("/api/settings").status, 404);
    assert_eq!(s.get("/healthz").status, 200, "the health check is always there");
}

#[test]
fn web_dir_serves_from_disk_instead() {
    let dir = std::env::temp_dir().join(format!("coaster-queue-webdir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "from disk").unwrap();
    let server = Server { port: 0, web_dir: Some(dir.clone()), ..Server::default() };
    let s = TestServer::start_with(API_AND_WEB, server);
    assert_eq!(s.get("/").text(), "from disk");
    assert_eq!(s.get("/app.js").status, 404, "listed, but missing from the directory");
    // Only the listed files are served, so a path can't walk out of the directory.
    assert_eq!(s.get("/../Cargo.toml").status, 404);
    assert_eq!(s.get("/%2e%2e/Cargo.toml").status, 404);
    std::fs::remove_dir_all(dir).unwrap();
}

// ---- malformed and abusive requests -----------------------------------------

#[test]
fn a_malformed_request_line_is_a_400() {
    let s = TestServer::start(API);
    assert_eq!(s.raw(b"\r\n\r\n").status, 400);
    assert_eq!(s.raw(b"GET\r\n\r\n").status, 400);
}

#[test]
fn an_oversized_body_is_refused() {
    let s = TestServer::start(API);
    let huge = "x".repeat(17 * 1024);
    let head = format!("PATCH /api/settings HTTP/1.1\r\nContent-Length: {}\r\n\r\n", huge.len());
    assert_eq!(s.raw(head.as_bytes()).status, 400, "refused from the header alone, body unread");
}

#[test]
fn the_query_string_and_fragment_do_not_affect_the_route() {
    let s = TestServer::start(API);
    assert_eq!(s.get("/healthz?x=1").status, 200);
    assert_eq!(s.get("/healthz#frag").status, 200);
}

#[test]
fn responses_carry_the_safety_headers() {
    let r = TestServer::start(API).get("/api/settings");
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(r.header("cache-control"), Some("no-cache"));
}

/// Beyond `max_connections` open connections, new clients are told to come back later
/// instead of the server growing a thread each.
#[test]
fn too_many_connections_get_a_503() {
    let s = TestServer::start_with(API, Server { port: 0, max_connections: 1, ..Server::default() });
    let _held = TcpStream::connect(s.addr).unwrap(); // occupies the only slot; sends nothing
    thread::sleep(Duration::from_millis(100)); // let the server accept it first
    let r = s.get("/healthz");
    assert_eq!((r.status, r.text().as_str()), (503, "busy"));
}
