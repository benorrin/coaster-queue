mod queue;
mod web;

use queue::{fetch, Ride, PARKS};
use std::{
    env,
    io::{self, Write},
    process,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";

const BAR_WIDTH: usize = 8;
const BAR_MAX_MINS: u32 = 60;
const WIDE_LAYOUT_MIN_COLS: usize = 90;

// ---- terminal ----------------------------------------------------------

struct Term {
    orig: libc::termios,
}

impl Term {
    fn enter() -> Term {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            libc::tcgetattr(0, &mut t);
            let orig = t;
            libc::cfmakeraw(&mut t);
            t.c_cc[libc::VMIN] = 0;
            t.c_cc[libc::VTIME] = 1; // 100ms read timeout doubles as the tick
            libc::tcsetattr(0, libc::TCSANOW, &t);
            print!("\x1b[?1049h\x1b[?25l");
            let _ = io::stdout().flush();
            Term { orig }
        }
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &self.orig) };
        print!("\x1b[?25h\x1b[?1049l");
        let _ = io::stdout().flush();
    }
}

fn term_size() -> (usize, usize) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            (ws.ws_col as usize, ws.ws_row as usize)
        } else {
            (80, 24)
        }
    }
}

fn read_key() -> Option<u8> {
    let mut b = 0u8;
    let n = unsafe { libc::read(0, &mut b as *mut u8 as *mut libc::c_void, 1) };
    (n == 1).then_some(b)
}

// ---- state -------------------------------------------------------------

#[derive(Default)]
struct ParkState {
    rides: Option<Vec<Ride>>,
    error: Option<String>,
    fetched: Option<Instant>,
    in_flight: bool,
}

type FetchResult = (usize, Result<Vec<Ride>, String>);

fn refresh(states: &mut [ParkState], parks: &[(&str, &str, u32)], tx: &mpsc::Sender<FetchResult>) {
    for (i, st) in states.iter_mut().enumerate() {
        if st.in_flight {
            continue;
        }
        st.in_flight = true;
        let (tx, id) = (tx.clone(), parks[i].2);
        thread::spawn(move || {
            let _ = tx.send((i, fetch(id)));
        });
    }
}

// ---- rendering ---------------------------------------------------------

fn fit(s: &str, n: usize) -> String {
    let len = s.chars().count();
    if len <= n {
        format!("{s}{}", " ".repeat(n - len))
    } else if n == 0 {
        String::new()
    } else {
        let mut out: String = s.chars().take(n - 1).collect();
        out.push('…');
        out
    }
}

fn wait_colour(wait: u32) -> &'static str {
    match wait {
        0..=20 => GREEN,
        21..=45 => YELLOW,
        _ => RED,
    }
}

fn bar(wait: u32) -> String {
    let filled = ((wait.min(BAR_MAX_MINS) as usize * BAR_WIDTH) + BAR_MAX_MINS as usize / 2)
        / BAR_MAX_MINS as usize;
    format!(
        "{}{}{DIM}{}{RESET}",
        wait_colour(wait),
        "█".repeat(filled),
        "░".repeat(BAR_WIDTH - filled)
    )
}

/// Body rows for one park; every row is exactly `iw` visible columns.
fn park_rows(st: &ParkState, iw: usize, show_closed: bool) -> Vec<String> {
    let msg = |text: &str, style: &str| format!("{style}{}{RESET}", fit(&format!(" {text}"), iw));
    let Some(rides) = &st.rides else {
        return vec![match &st.error {
            Some(e) => msg(&format!("error: {e}"), RED),
            None => msg("loading…", DIM),
        }];
    };

    let open: Vec<&Ride> = rides.iter().filter(|r| r.is_open).collect();
    let closed: Vec<&Ride> = rides.iter().filter(|r| !r.is_open).collect();
    // name column: 1 pad + name + 1 gap + bar + 1 gap + "999 min" + 1 pad
    let nw = iw.saturating_sub(1 + 1 + BAR_WIDTH + 1 + 7 + 1).max(6);

    let mut rows = Vec::new();
    if open.is_empty() {
        rows.push(msg("Closed — no rides operating", DIM));
    }
    for r in &open {
        rows.push(format!(
            " {} {} {}{:>3} min{RESET} ",
            fit(&r.name, nw),
            bar(r.wait_time),
            wait_colour(r.wait_time),
            r.wait_time
        ));
    }
    if show_closed {
        for r in &closed {
            rows.push(format!(
                "{DIM} {} {}{RESET}",
                fit(&r.name, iw.saturating_sub(10)),
                "closed  "
            ));
        }
    } else if !closed.is_empty() {
        rows.push(msg(&format!("{} closed · press a to list", closed.len()), DIM));
    }
    rows
}

fn panel(name: &str, st: &ParkState, w: usize, h: usize, show_closed: bool) -> Vec<String> {
    let iw = w.saturating_sub(2);
    let bh = h.saturating_sub(2);

    let stats = match &st.rides {
        Some(rides) => {
            let waits: Vec<u32> = rides.iter().filter(|r| r.is_open).map(|r| r.wait_time).collect();
            if waits.is_empty() {
                "closed".to_string()
            } else {
                let avg = waits.iter().sum::<u32>() / waits.len() as u32;
                format!("{} open · avg {avg}m · max {}m", waits.len(), waits.iter().max().unwrap())
            }
        }
        None => String::new(),
    };
    let (nlen, slen) = (name.chars().count(), stats.chars().count());
    let top = if w >= nlen + slen + 9 {
        let fill = w - (nlen + slen + 8);
        format!("┌─ {BOLD}{CYAN}{name}{RESET} {}{DIM} {stats}{RESET} ─┐", "─".repeat(fill))
    } else {
        format!("┌─ {BOLD}{CYAN}{}{RESET} ┐", fit(name, iw.saturating_sub(3)))
    };

    let mut body = park_rows(st, iw, show_closed);
    if body.len() > bh {
        let hidden = body.len() - bh + 1;
        body.truncate(bh.saturating_sub(1));
        body.push(format!("{DIM}{}{RESET}", fit(&format!(" … +{hidden} more"), iw)));
    }
    while body.len() < bh {
        body.push(" ".repeat(iw));
    }

    let mut out = vec![top];
    out.extend(body.into_iter().map(|r| format!("│{r}│")));
    out.push(format!("└{}┘", "─".repeat(iw)));
    out
}

fn render(
    parks: &[(&str, &str, u32)],
    states: &[ParkState],
    show_closed: bool,
    next_poll: Instant,
) -> String {
    let (cols, rows) = term_size();
    let body_h = rows.saturating_sub(2);
    let mut lines = vec![format!(
        " {BOLD}COASTER QUEUE{RESET}{DIM}  live ride wait times{RESET}"
    )];

    let n = parks.len();
    if n > 1 && cols >= WIDE_LAYOUT_MIN_COLS {
        let widths: Vec<usize> = (0..n).map(|i| cols / n + usize::from(i < cols % n)).collect();
        let panels: Vec<Vec<String>> = (0..n)
            .map(|i| panel(parks[i].1, &states[i], widths[i], body_h, show_closed))
            .collect();
        for r in 0..body_h {
            lines.push(panels.iter().map(|p| p[r].as_str()).collect::<String>());
        }
    } else {
        let each = body_h / n;
        for i in 0..n {
            lines.extend(panel(parks[i].1, &states[i], cols, each, show_closed));
        }
    }

    let now = Instant::now();
    let age = states.iter().filter_map(|s| s.fetched).max().map(|t| now - t);
    let mut foot = format!(
        " q quit · r refresh · a closed rides: {} · next poll {}s · updated {}",
        if show_closed { "shown" } else { "hidden" },
        next_poll.saturating_duration_since(now).as_secs(),
        age.map_or("never".into(), |a| format!("{}s ago", a.as_secs())),
    );
    let mut style = DIM;
    if let Some(e) = states.iter().find_map(|s| s.error.as_ref()) {
        foot = format!(" fetch failed ({e}) — showing last data · retrying");
        style = RED;
    }
    lines.truncate(rows.saturating_sub(1));
    while lines.len() < rows.saturating_sub(1) {
        lines.push(String::new());
    }
    lines.push(format!("{style}{}{RESET}", fit(&foot, cols)));

    let mut frame = String::from("\x1b[H");
    frame.push_str(&lines.into_iter().map(|l| format!("{l}\x1b[K")).collect::<Vec<_>>().join("\r\n"));
    frame
}

fn serve(args: impl Iterator<Item = String>) {
    let mut cfg = web::Config {
        bind: "127.0.0.1".into(),
        port: 8080,
        interval: 30,
        web_dir: None,
    };
    let mut args = args;
    while let Some(a) = args.next() {
        let mut value = |name: &str| {
            args.next().unwrap_or_else(|| {
                eprintln!("{name} needs a value");
                process::exit(2);
            })
        };
        match a.as_str() {
            "-b" | "--bind" => cfg.bind = value("--bind"),
            "-p" | "--port" => {
                cfg.port = value("--port").parse().unwrap_or_else(|_| {
                    eprintln!("--port needs a number");
                    process::exit(2);
                })
            }
            "-i" | "--interval" => cfg.interval = value("--interval").parse().unwrap_or(30).max(10),
            "--web-dir" => cfg.web_dir = Some(value("--web-dir").into()),
            "-h" | "--help" => {
                println!("usage: coaster-queue serve [-b|--bind ADDR] [-p|--port PORT] [-i|--interval SECS]");
                println!("                          [--web-dir DIR]");
                println!("serves the LED board at http://ADDR:PORT (default 127.0.0.1:8080)");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                process::exit(2);
            }
        }
    }
    if let Err(e) = web::run(cfg) {
        eprintln!("server error: {e}");
        process::exit(1);
    }
}

fn main() {
    let mut raw = env::args().skip(1).peekable();
    if raw.peek().map(String::as_str) == Some("serve") {
        raw.next();
        return serve(raw);
    }
    let (mut selected, mut interval, mut show_closed) = (Vec::new(), 30u64, false);
    let mut args = raw;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-a" | "--all" => show_closed = true,
            "-i" | "--interval" => {
                interval = args.next().and_then(|s| s.parse().ok()).unwrap_or(30).max(10)
            }
            "-h" | "--help" => {
                println!("usage: coaster-queue [alton] [thorpe] [-a|--all] [-i|--interval SECS]");
                println!("       coaster-queue serve --help");
                println!("keys:  q quit · r refresh now · a toggle closed rides");
                return;
            }
            other => match PARKS.iter().find(|p| p.0 == other) {
                Some(p) => selected.push(*p),
                None => {
                    eprintln!("unknown argument: {other}");
                    process::exit(2);
                }
            },
        }
    }
    if selected.is_empty() {
        selected = PARKS.to_vec();
    }

    let mut states: Vec<ParkState> = selected.iter().map(|_| ParkState::default()).collect();
    let (tx, rx) = mpsc::channel::<FetchResult>();
    let every = Duration::from_secs(interval);
    let mut next_poll = Instant::now();
    let mut last_frame = String::new();

    let _term = Term::enter();
    loop {
        while let Ok((i, res)) = rx.try_recv() {
            let st = &mut states[i];
            st.in_flight = false;
            match res {
                Ok(rides) => {
                    st.rides = Some(rides);
                    st.error = None;
                    st.fetched = Some(Instant::now());
                }
                Err(e) => st.error = Some(e),
            }
        }
        if Instant::now() >= next_poll {
            refresh(&mut states, &selected, &tx);
            next_poll = Instant::now() + every;
        }

        let frame = render(&selected, &states, show_closed, next_poll);
        if frame != last_frame {
            let mut out = io::stdout().lock();
            let _ = out.write_all(frame.as_bytes());
            let _ = out.flush();
            last_frame = frame;
        }

        match read_key() {
            Some(b'q' | b'Q' | 3 | 27) => break,
            Some(b'r' | b'R') => next_poll = Instant::now(),
            Some(b'a' | b'A') => show_closed = !show_closed,
            _ => {}
        }
    }
}
