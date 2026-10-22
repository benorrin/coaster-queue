//! The main display. For now it draws to the terminal; it is kept separate from the data
//! and settings code so an LED matrix driver can take its place later.

use crate::groups::Filter;
use crate::queue::Ride;
use crate::settings::{Patch, Settings};
use crate::store::{now_secs, Shared};
use std::{
    io::{self, Write},
    sync::atomic::Ordering,
};

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";

/// Width of the queue bar, in cells, and the wait it is full at.
const BAR_WIDTH: usize = 8;
const BAR_MAX_MINS: u32 = 60;
/// Parks sit side by side on terminals at least this wide, each at least `PANEL_MIN_COLS`;
/// otherwise they're stacked, each needing `PANEL_MIN_ROWS`.
const WIDE_LAYOUT_MIN_COLS: usize = 90;
const PANEL_MIN_COLS: usize = 40;
const PANEL_MIN_ROWS: usize = 6;

// ---- terminal ---------------------------------------------------------------

/// Puts the terminal into raw mode on the alternate screen, and restores it on drop.
struct Term {
    orig: libc::termios,
}

impl Term {
    fn enter() -> Term {
        // SAFETY: plain termios calls on stdin with a zeroed struct that tcgetattr fills in.
        let orig = unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            libc::tcgetattr(0, &mut t);
            let orig = t;
            libc::cfmakeraw(&mut t);
            t.c_cc[libc::VMIN] = 0;
            t.c_cc[libc::VTIME] = 1; // reads time out after 100ms, which doubles as the tick
            libc::tcsetattr(0, libc::TCSANOW, &t);
            orig
        };
        print!("\x1b[?1049h\x1b[?25l"); // alternate screen, hide the cursor
        let _ = io::stdout().flush();
        Term { orig }
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        // SAFETY: restores the attributes saved in `enter`.
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &self.orig) };
        print!("\x1b[?25h\x1b[?1049l");
        let _ = io::stdout().flush();
    }
}

/// The terminal's (columns, rows), or 80x24 if it can't be asked.
fn term_size() -> (usize, usize) {
    // SAFETY: TIOCGWINSZ fills in the zeroed winsize struct.
    let ws = unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        (libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0).then_some(ws)
    };
    match ws {
        Some(ws) if ws.ws_col > 0 && ws.ws_row > 0 => (usize::from(ws.ws_col), usize::from(ws.ws_row)),
        _ => (80, 24),
    }
}

/// Wait up to 100ms for a key press.
fn read_key() -> Option<u8> {
    let mut byte = 0u8;
    // SAFETY: reads at most one byte into a valid one-byte buffer.
    let n = unsafe { libc::read(0, std::ptr::from_mut(&mut byte).cast(), 1) };
    (n == 1).then_some(byte)
}

// ---- drawing ----------------------------------------------------------------

/// What one park's panel shows.
struct Pane {
    name: String,
    /// None until the park's first fetch succeeds.
    rides: Option<Vec<Ride>>,
    error: Option<String>,
    fetched_at: Option<u64>,
}

/// Pad or cut `s` to exactly `n` characters, ending a cut string with an ellipsis.
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

/// A queue-length bar, filled in proportion to the wait.
fn bar(wait: u32) -> String {
    let max = BAR_MAX_MINS as usize;
    let filled = (wait.min(BAR_MAX_MINS) as usize * BAR_WIDTH + max / 2) / max;
    format!("{}{}{DIM}{}{RESET}", wait_colour(wait), "█".repeat(filled), "░".repeat(BAR_WIDTH - filled))
}

/// Body rows for one park; every row is exactly `iw` visible columns.
fn park_rows(pane: &Pane, iw: usize, show_closed: bool) -> Vec<String> {
    let message = |text: &str, style: &str| format!("{style}{}{RESET}", fit(&format!(" {text}"), iw));
    let Some(rides) = &pane.rides else {
        return vec![match &pane.error {
            Some(e) => message(&format!("error: {e}"), RED),
            None => message("loading…", DIM),
        }];
    };

    let (open, closed): (Vec<&Ride>, Vec<&Ride>) = rides.iter().partition(|r| r.is_open);
    // Name column: 1 pad + name + 1 gap + bar + 1 gap + "999 min" + 1 pad.
    let name_width = iw.saturating_sub(1 + 1 + BAR_WIDTH + 1 + 7 + 1).max(6);

    let mut rows = Vec::new();
    if open.is_empty() {
        rows.push(message("Closed — no rides operating", DIM));
    }
    for r in open {
        rows.push(format!(
            " {} {} {}{:>3} min{RESET} ",
            fit(&r.name, name_width),
            bar(r.wait_time),
            wait_colour(r.wait_time),
            r.wait_time
        ));
    }
    if show_closed {
        // 1 pad + name + 1 gap + "closed  " (8) = iw
        rows.extend(
            closed.iter().map(|r| format!("{DIM} {} closed  {RESET}", fit(&r.name, iw.saturating_sub(10)))),
        );
    } else if !closed.is_empty() {
        rows.push(message(&format!("{} closed · press a to list", closed.len()), DIM));
    }
    rows
}

/// "3 open · avg 12m · max 40m", or "closed", for a panel's title bar.
fn park_stats(rides: &[Ride]) -> String {
    let waits: Vec<u32> = rides.iter().filter(|r| r.is_open).map(|r| r.wait_time).collect();
    let Some(max) = waits.iter().max() else { return "closed".to_string() };
    let avg = waits.iter().sum::<u32>() / waits.len() as u32;
    format!("{} open · avg {avg}m · max {max}m", waits.len())
}

/// A bordered panel `w` columns by `h` rows for one park.
fn panel(pane: &Pane, w: usize, h: usize, show_closed: bool) -> Vec<String> {
    let inner = w.saturating_sub(2);
    let body_height = h.saturating_sub(2);

    let stats = pane.rides.as_deref().map(park_stats).unwrap_or_default();
    let (name_len, stats_len) = (pane.name.chars().count(), stats.chars().count());
    let top = if w >= name_len + stats_len + 9 {
        let fill = w - (name_len + stats_len + 8);
        format!("┌─ {BOLD}{CYAN}{}{RESET} {}{DIM} {stats}{RESET} ─┐", pane.name, "─".repeat(fill))
    } else {
        format!("┌─ {BOLD}{CYAN}{}{RESET} ┐", fit(&pane.name, inner.saturating_sub(3)))
    };

    let mut body = park_rows(pane, inner, show_closed);
    if body.len() > body_height {
        let hidden = body.len() - body_height + 1;
        body.truncate(body_height.saturating_sub(1));
        body.push(format!("{DIM}{}{RESET}", fit(&format!(" … +{hidden} more"), inner)));
    }
    body.resize(body_height, " ".repeat(inner));

    let mut out = vec![top];
    out.extend(body.into_iter().map(|row| format!("│{row}│")));
    out.push(format!("└{}┘", "─".repeat(inner)));
    out
}

/// Which screen of parks is up, and how many there are.
struct Screens {
    current: usize,
    total: usize,
}

/// Everything one frame needs besides the terminal size.
struct Frame<'a> {
    panes: &'a [Pane],
    side_by_side: bool,
    show_closed: bool,
    /// The groups the rides are restricted to, shown in the title.
    groups: String,
    screens: Screens,
}

fn title_line(groups: &str) -> String {
    let mut title = format!(" {BOLD}COASTER QUEUE{RESET}{DIM}  live ride wait times{RESET}");
    if !groups.is_empty() {
        title.push_str(&format!("{DIM} · {groups}{RESET}"));
    }
    title
}

/// The key hints and data age, then the page counter. The counter is always shown; it's the
/// message beside it that gets cut to fit.
fn footer_line(frame: &Frame, cols: usize) -> String {
    let now = now_secs();
    let age = frame.panes.iter().filter_map(|p| p.fetched_at).max().map(|t| now.saturating_sub(t));
    let (message, style) = match frame.panes.iter().find_map(|p| p.error.as_ref()) {
        Some(e) => (format!(" fetch failed ({e}) — showing last data · retrying"), RED),
        None => (
            format!(
                " q quit · r refresh · a closed rides: {} · updated {}",
                if frame.show_closed { "shown" } else { "hidden" },
                age.map_or("never".into(), |a| format!("{a}s ago")),
            ),
            DIM,
        ),
    };
    let counter = format!(" page {}/{} ", frame.screens.current + 1, frame.screens.total);
    let room = cols.saturating_sub(counter.chars().count());
    format!("{style}{}{counter}{RESET}", fit(&message, room))
}

/// Draw one screen: a title, the panels, and a footer, as a string ready to print.
fn render(frame: &Frame) -> String {
    let (cols, rows) = term_size();
    let body_height = rows.saturating_sub(2);
    let mut lines = vec![title_line(&frame.groups)];

    let n = frame.panes.len().max(1);
    if frame.side_by_side && n > 1 {
        // Share the width out as evenly as possible, any remainder going to the first panels.
        let panels: Vec<Vec<String>> = frame
            .panes
            .iter()
            .enumerate()
            .map(|(i, pane)| {
                panel(pane, cols / n + usize::from(i < cols % n), body_height, frame.show_closed)
            })
            .collect();
        lines.extend((0..body_height).map(|r| panels.iter().map(|p| p[r].as_str()).collect::<String>()));
    } else {
        let each = body_height / n;
        lines.extend(frame.panes.iter().flat_map(|pane| panel(pane, cols, each, frame.show_closed)));
    }

    lines.truncate(rows.saturating_sub(1));
    lines.resize(rows.saturating_sub(1), String::new());
    lines.push(footer_line(frame, cols));

    let mut out = String::from("\x1b[H"); // cursor home
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        out.push_str(line);
        out.push_str("\x1b[K"); // clear the rest of the row
    }
    out
}

// ---- the display loop -------------------------------------------------------

/// One pane per configured park, from the store.
fn build_panes(shared: &Shared, settings: &Settings) -> Vec<Pane> {
    let filter = shared.filter(&settings.show).unwrap_or_else(|_| Filter::all());
    settings
        .parks
        .iter()
        .map(|slug| match shared.store.view(slug, &filter) {
            Some(view) => Pane {
                name: view.name,
                rides: view.fetched_at.map(|_| view.rides),
                error: view.error,
                fetched_at: view.fetched_at,
            },
            None => Pane {
                name: slug.clone(),
                rides: None,
                error: Some(format!("unknown park {slug}")),
                fetched_at: None,
            },
        })
        .collect()
}

/// How many parks fit on one screen, and whether they go side by side.
fn parks_per_screen(cols: usize, rows: usize) -> (usize, bool) {
    let across = (cols / PANEL_MIN_COLS).max(1);
    if cols >= WIDE_LAYOUT_MIN_COLS && across >= 2 {
        (across, true)
    } else {
        ((rows.saturating_sub(2) / PANEL_MIN_ROWS).max(1), false)
    }
}

/// What a redraw would depend on; a frame is only drawn when this changes.
#[derive(PartialEq)]
struct Signature {
    data: u64,
    settings: u64,
    size: (usize, usize),
    /// The wall clock in seconds, for the "updated Ns ago" text and the rotation between screens.
    second: u64,
}

/// Run the display until the user quits. Redraws only when something changed: new data,
/// new settings, a resized terminal, or the clock ticking over a second.
pub fn run(shared: &Shared) {
    let _term = Term::enter();
    let mut last_sig = None;
    let mut last_frame = String::new();

    loop {
        // Blocks for up to 100ms, which is also the loop's tick.
        match read_key() {
            Some(b'q' | b'Q' | 3 | 27) => break, // 3 is Ctrl-C, 27 is Escape
            Some(b'r' | b'R') => shared.store.refresh(),
            Some(b'a' | b'A') => {
                let show_closed = !shared.settings().show_closed;
                let _ = shared.update(Patch { show_closed: Some(show_closed), ..Patch::default() });
            }
            _ => {}
        }

        let second = now_secs();
        let size = term_size();
        let sig =
            Signature { data: shared.store.generation(), settings: shared.settings_gen(), size, second };
        if last_sig.as_ref() == Some(&sig) {
            continue;
        }
        last_sig = Some(sig);

        let settings = shared.settings();
        let panes = build_panes(shared, &settings);

        // As many parks per screen as fit; more than that take turns, one screen per dwell.
        let (per_screen, side_by_side) = parks_per_screen(size.0, size.1);
        let total = panes.len().div_ceil(per_screen).max(1);
        let current = (second / u64::from(settings.dwell)) as usize % total;
        let start = (current * per_screen).min(panes.len());
        let end = ((current + 1) * per_screen).min(panes.len());

        let frame = render(&Frame {
            panes: &panes[start..end],
            side_by_side,
            show_closed: settings.show_closed,
            groups: settings.show.join(", "),
            screens: Screens { current, total },
        });
        if frame != last_frame {
            let mut out = io::stdout().lock();
            let _ = out.write_all(frame.as_bytes());
            let _ = out.flush();
            last_frame = frame;
        }
    }
    shared.quit.store(true, Ordering::Relaxed);
}
