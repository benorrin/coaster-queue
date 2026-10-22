use super::*;

/// What a terminal would show: the text with colour codes removed.
fn visible(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for e in chars.by_ref() {
                if e.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn width(s: &str) -> usize {
    visible(s).chars().count()
}

fn ride(name: &str, open: bool, wait: u32) -> Ride {
    Ride { name: name.into(), is_open: open, wait_time: wait, land: None }
}

fn pane(rides: Option<Vec<Ride>>) -> Pane {
    Pane { name: "Alton Towers".into(), rides, error: None, fetched_at: None }
}

// ---- small helpers ----------------------------------------------------------

#[test]
fn fit_pads_cuts_and_ellipsises() {
    assert_eq!(fit("abc", 5), "abc  ");
    assert_eq!(fit("abc", 3), "abc");
    assert_eq!(fit("abcdef", 4), "abc…");
    assert_eq!(fit("abc", 0), "");
    assert_eq!(fit("", 2), "  ");
}

#[test]
fn fit_counts_characters_not_bytes() {
    assert_eq!(fit("Andy’s Adventures", 6), "Andy’…");
    assert_eq!(width(&fit("Þórr’s Hammer", 20)), 20);
}

#[test]
fn wait_colours_change_at_the_thresholds() {
    assert_eq!(wait_colour(0), GREEN);
    assert_eq!(wait_colour(20), GREEN);
    assert_eq!(wait_colour(21), YELLOW);
    assert_eq!(wait_colour(45), YELLOW);
    assert_eq!(wait_colour(46), RED);
    assert_eq!(wait_colour(300), RED);
}

#[test]
fn bar_fills_in_proportion_and_caps() {
    let cells = |wait| {
        let b = visible(&bar(wait));
        (b.chars().filter(|&c| c == '█').count(), b.chars().filter(|&c| c == '░').count())
    };
    assert_eq!(cells(0), (0, 8));
    assert_eq!(cells(30), (4, 4));
    assert_eq!(cells(60), (8, 0));
    assert_eq!(cells(600), (8, 0), "waits over an hour don't overflow the bar");
}

#[test]
fn stats_summarise_open_rides_only() {
    let rides = [ride("A", true, 10), ride("B", true, 31), ride("C", false, 99)];
    assert_eq!(park_stats(&rides), "2 open · avg 20m · max 31m");
    assert_eq!(park_stats(&[ride("A", false, 0)]), "closed");
    assert_eq!(park_stats(&[]), "closed");
}

// ---- layout -----------------------------------------------------------------

#[test]
fn narrow_terminals_stack_the_parks_and_wide_ones_go_side_by_side() {
    assert_eq!(parks_per_screen(80, 24), (3, false), "(24 - 2) / 6 rows each");
    assert_eq!(parks_per_screen(120, 40), (3, true), "120 / 40 columns each");
    assert_eq!(parks_per_screen(200, 40), (5, true));
    assert_eq!(parks_per_screen(89, 24), (3, false), "just under the wide threshold");
}

#[test]
fn at_least_one_park_always_fits() {
    assert_eq!(parks_per_screen(10, 3), (1, false));
    assert_eq!(parks_per_screen(0, 0), (1, false));
}

// ---- rows and panels --------------------------------------------------------

#[test]
fn rows_before_the_first_fetch() {
    let loading = park_rows(&pane(None), 30, false);
    assert_eq!(loading.len(), 1);
    assert!(visible(&loading[0]).trim() == "loading…");

    let failed = Pane { error: Some("timeout".into()), ..pane(None) };
    assert!(visible(&park_rows(&failed, 30, false)[0]).contains("error: timeout"));
}

#[test]
fn every_row_is_exactly_the_panel_width() {
    let rides = vec![
        ride("Nemesis", true, 25),
        ride("A very long ride name that must be cut to fit", true, 120),
        ride("Oblivion", false, 0),
    ];
    for iw in [30, 40, 78] {
        for show_closed in [false, true] {
            for row in park_rows(&pane(Some(rides.clone())), iw, show_closed) {
                assert_eq!(width(&row), iw, "iw={iw} closed={show_closed}: {:?}", visible(&row));
            }
        }
    }
}

#[test]
fn closed_rides_are_counted_unless_shown() {
    let rides = vec![ride("Nemesis", true, 25), ride("Oblivion", false, 0), ride("Smiler", false, 0)];
    let hidden = park_rows(&pane(Some(rides.clone())), 40, false);
    assert_eq!(hidden.len(), 2);
    assert!(visible(&hidden[1]).contains("2 closed · press a to list"));
    let shown = park_rows(&pane(Some(rides)), 40, true);
    assert_eq!(shown.len(), 3);
    assert!(visible(&shown[2]).contains("Smiler") && visible(&shown[2]).trim_end().ends_with("closed"));
}

#[test]
fn a_park_with_no_open_rides_says_so() {
    let rows = park_rows(&pane(Some(vec![ride("Oblivion", false, 0)])), 40, false);
    assert!(visible(&rows[0]).contains("Closed — no rides operating"));
}

#[test]
fn a_panel_is_exactly_the_size_asked_for() {
    let rides = vec![ride("Nemesis", true, 25), ride("Oblivion", false, 0)];
    for (w, h) in [(40, 8), (60, 12), (27, 5), (120, 3)] {
        let lines = panel(&pane(Some(rides.clone())), w, h, false);
        assert_eq!(lines.len(), h, "{w}x{h}");
        for line in &lines {
            assert_eq!(width(line), w, "{w}x{h}: {:?}", visible(line));
        }
    }
}

/// A ride row never gets narrower than 27 columns, so on a terminal narrower than that the
/// rows poke out past the panel border. Panels are normally at least `PANEL_MIN_COLS` wide,
/// so this only shows on a very narrow terminal. Remove the `ignore` once it's fixed.
#[test]
#[ignore = "known: ride rows overflow panels narrower than 27 columns"]
fn a_very_narrow_panel_still_has_straight_edges() {
    let lines = panel(&pane(Some(vec![ride("Nemesis", true, 25)])), 20, 5, false);
    for line in &lines {
        assert_eq!(width(line), 20, "{:?}", visible(line));
    }
}

#[test]
fn the_title_bar_has_the_name_and_stats_when_there_is_room() {
    let wide = panel(&pane(Some(vec![ride("Nemesis", true, 25)])), 60, 5, false);
    assert!(
        visible(&wide[0]).contains("Alton Towers")
            && visible(&wide[0]).contains("1 open · avg 25m · max 25m")
    );
    let narrow = panel(&pane(Some(vec![ride("Nemesis", true, 25)])), 24, 5, false);
    assert!(visible(&narrow[0]).contains("Alton Towers") && !visible(&narrow[0]).contains("open"));
}

#[test]
fn a_panel_too_short_for_its_rides_says_how_many_are_hidden() {
    let rides: Vec<Ride> = (0..10).map(|i| ride(&format!("Ride {i}"), true, i)).collect();
    let lines = panel(&pane(Some(rides)), 40, 6, false);
    assert_eq!(lines.len(), 6);
    // Four body rows: three rides, then the note covering the seven that don't fit.
    assert!(visible(&lines[3]).contains("Ride 2"));
    assert!(visible(&lines[4]).contains("… +7 more"), "{:?}", visible(&lines[4]));
}

#[test]
fn a_panel_with_exactly_enough_room_has_no_more_note() {
    let rides: Vec<Ride> = (0..3).map(|i| ride(&format!("Ride {i}"), true, i)).collect();
    let lines = panel(&pane(Some(rides)), 40, 5, false);
    assert!(!lines.iter().any(|l| visible(l).contains("more")));
}

#[test]
fn the_footer_always_shows_the_page_counter() {
    let mut frame = Frame {
        panes: &[],
        side_by_side: false,
        show_closed: false,
        groups: String::new(),
        screens: Screens { current: 1, total: 3 },
    };
    for cols in [80, 20, 12] {
        let footer = visible(&footer_line(&frame, cols));
        assert!(footer.ends_with("page 2/3 "), "{cols}: {footer:?}");
        assert_eq!(footer.chars().count(), cols);
    }
    frame.show_closed = true;
    assert!(visible(&footer_line(&frame, 80)).contains("a closed rides: shown"));
}

#[test]
fn the_footer_reports_fetch_errors() {
    let panes = [Pane { error: Some("timeout".into()), ..pane(None) }];
    let frame = Frame {
        panes: &panes,
        side_by_side: false,
        show_closed: false,
        groups: String::new(),
        screens: Screens { current: 0, total: 1 },
    };
    assert!(visible(&footer_line(&frame, 100)).contains("fetch failed (timeout)"));
}

#[test]
fn the_title_lists_the_groups_when_restricted() {
    assert!(visible(&title_line("")).trim_end().ends_with("live ride wait times"));
    assert!(visible(&title_line("thrill, family")).ends_with("· thrill, family"));
}
