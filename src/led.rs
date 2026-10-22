//! The main display. For now it draws to the terminal; it is kept separate from the
//! data and settings code so an LED matrix driver can take its place later.

use crate::queue::Ride;
use crate::store::Shared;
use std::{
    io::{self, Write},
    sync::{atomic::Ordering, Arc},
    time::{SystemTime, UNIX_EPOCH},
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

/// What one park panel shows.
struct Pane {
    name: String,
    rides: Option<Vec<Ride>>,
    error: Option<String>,
    fetched_at: Option<u64>,
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
fn park_rows(st: &Pane, iw: usize, show_closed: bool) -> Vec<String> {
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

fn panel(name: &str, st: &Pane, w: usize, h: usize, show_closed: bool) -> Vec<String> {
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


/// Draw one screen: a header, the panels for `parks`, and a footer.
fn render(
    panes: &[Pane],
    shows: &str,
    show_closed: bool,
    page: Option<(usize, usize)>,
    wide: bool,
) -> String {
    let (cols, rows) = term_size();
    let body_h = rows.saturating_sub(2);
    let mut title = format!(" {BOLD}COASTER QUEUE{RESET}{DIM}  live ride wait times{RESET}");
    if !shows.is_empty() {
        title.push_str(&format!("{DIM} · {shows}{RESET}"));
    }
    let mut lines = vec![title];

    let n = panes.len().max(1);
    if wide && n > 1 {
        let widths: Vec<usize> = (0..n).map(|i| cols / n + usize::from(i < cols % n)).collect();
        let panels: Vec<Vec<String>> =
            (0..n).map(|i| panel(&panes[i].name, &panes[i], widths[i], body_h, show_closed)).collect();
        for r in 0..body_h {
            lines.push(panels.iter().map(|p| p[r].as_str()).collect::<String>());
        }
    } else {
        let each = body_h / n;
        for p in panes {
            lines.extend(panel(&p.name, p, cols, each, show_closed));
        }
    }

    let now = now_secs();
    let age = panes.iter().filter_map(|p| p.fetched_at).max().map(|t| now.saturating_sub(t));
    let mut foot = format!(
        " q quit · r refresh · a closed rides: {} · updated {}",
        if show_closed { "shown" } else { "hidden" },
        age.map_or("never".into(), |a| format!("{a}s ago")),
    );
    if let Some((i, of)) = page {
        foot.push_str(&format!(" · page {}/{of}", i + 1));
    }
    let mut style = DIM;
    if let Some(e) = panes.iter().find_map(|p| p.error.as_ref()) {
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

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Run the display until the user quits. Redraws only when something changed: new data,
/// new settings, a resized terminal, or the clock ticking over a second.
pub fn run(shared: Arc<Shared>) {
    let _term = Term::enter();
    let mut last_sig = (u64::MAX, u64::MAX, (0, 0), 0u64);
    let mut last_frame = String::new();

    loop {
        // Blocks for up to 100ms, which is also the loop's tick.
        match read_key() {
            Some(b'q' | b'Q' | 3 | 27) => break,
            Some(b'r' | b'R') => shared.store.refresh(),
            Some(b'a' | b'A') => {
                let show = !shared.settings().show_closed;
                let _ = shared.update(crate::settings::Patch { show_closed: Some(show), ..Default::default() });
            }
            _ => {}
        }

        let settings = shared.settings();
        let secs = now_secs();
        let sig = (shared.store.generation(), shared.settings_gen(), term_size(), secs);
        if sig == last_sig {
            continue;
        }
        last_sig = sig;

        let filter = shared.filter(&settings.show).unwrap_or_else(|_| crate::groups::Filter::all());
        let panes: Vec<Pane> = settings
            .parks
            .iter()
            .map(|slug| match shared.store.view(slug, &filter) {
                Some(v) => Pane { name: v.name, rides: v.fetched_at.map(|_| v.rides), error: v.error, fetched_at: v.fetched_at },
                None => Pane { name: slug.clone(), rides: None, error: Some(format!("unknown park {slug}")), fetched_at: None },
            })
            .collect();

        // As many parks per screen as fit; more than that take turns, one screen per dwell.
        let (cols, rows) = term_size();
        let wide_fit = (cols / 40).max(1);
        let wide = cols >= WIDE_LAYOUT_MIN_COLS && wide_fit >= 2;
        let per = if wide { wide_fit } else { (rows.saturating_sub(2) / 6).max(1) };
        let screens = panes.len().div_ceil(per).max(1);
        let current = (secs / u64::from(settings.dwell.max(1))) as usize % screens;
        let shown = &panes[(current * per).min(panes.len())..((current + 1) * per).min(panes.len())];

        let shows = settings.show.join(", ");
        let frame = render(shown, &shows, settings.show_closed, (screens > 1).then_some((current, screens)), wide);
        if frame != last_frame {
            let mut out = io::stdout().lock();
            let _ = out.write_all(frame.as_bytes());
            let _ = out.flush();
            last_frame = frame;
        }
    }
    shared.quit.store(true, Ordering::Relaxed);
}
