use super::*;
use crate::groups::GroupDef;

const SLOW: Duration = Duration::from_secs(3600);

fn slugs(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn ride(name: &str, open: bool, wait: u32) -> Ride {
    Ride { name: name.into(), is_open: open, wait_time: wait, land: None }
}

fn weather(temp_c: f64) -> Weather {
    Weather { temp_c, feels_c: temp_c, code: 0, wind_mph: 0.0, rain_mm: 0.0 }
}

/// The parks a plan would fetch, as (slug, queue, weather), sorted.
fn planned(jobs: &[Job]) -> Vec<(&str, bool, bool)> {
    let mut out: Vec<_> = jobs.iter().map(|j| (j.park.slug.as_str(), j.queue, j.weather)).collect();
    out.sort();
    out
}

fn shared() -> Arc<Shared> {
    Shared::new(Settings::default(), Groups::builtin())
}

// ---- remaining --------------------------------------------------------------

#[test]
fn remaining_is_zero_when_never_done_or_overdue() {
    let now = Instant::now();
    let every = Duration::from_secs(60);
    assert_eq!(remaining(None, every, now), Duration::ZERO);
    assert_eq!(remaining(Some(now - Duration::from_secs(60)), every, now), Duration::ZERO);
    assert_eq!(remaining(Some(now - Duration::from_secs(90)), every, now), Duration::ZERO);
    assert_eq!(remaining(Some(now - Duration::from_secs(45)), every, now), Duration::from_secs(15));
}

// ---- planning ---------------------------------------------------------------

#[test]
fn first_plan_fetches_queues_and_weather_for_configured_parks_only() {
    let store = Store::new();
    let (jobs, _) = store.plan(&slugs(&["alton-towers"]), SLOW);
    assert_eq!(planned(&jobs), [("alton-towers", true, true)]);
}

#[test]
fn nothing_is_due_straight_after_a_fetch() {
    let store = Store::new();
    let configured = slugs(&["alton-towers"]);
    store.plan(&configured, SLOW);
    store.record("alton-towers", Some(Ok(vec![])), Some(Ok(weather(10.0))));
    let (jobs, sleep) = store.plan(&configured, Duration::from_secs(60));
    assert!(jobs.is_empty());
    assert!(sleep <= Duration::from_secs(60) && sleep > Duration::ZERO, "{sleep:?}");
}

/// Regression: a failing park is retried once per interval, not in a tight loop.
#[test]
fn a_failed_fetch_is_not_retried_until_the_interval_passes() {
    let store = Store::new();
    let configured = slugs(&["alton-towers"]);
    store.plan(&configured, SLOW);
    store.record("alton-towers", Some(Err("boom".into())), Some(Err("boom".into())));
    let (jobs, _) = store.plan(&configured, SLOW);
    assert!(jobs.is_empty(), "queue and weather were both just attempted");
}

#[test]
fn a_refresh_makes_queues_due_again_but_not_weather() {
    let store = Store::new();
    let configured = slugs(&["alton-towers"]);
    store.plan(&configured, SLOW);
    store.record("alton-towers", Some(Ok(vec![])), Some(Ok(weather(10.0))));
    store.refresh();
    let (jobs, _) = store.plan(&configured, SLOW);
    assert_eq!(planned(&jobs), [("alton-towers", true, false)]);
}

#[test]
fn a_park_a_client_asked_for_is_polled_beside_the_configured_ones() {
    let store = Store::new();
    store.want(&slugs(&["thorpe-park", "not-a-park"]));
    let (jobs, _) = store.plan(&slugs(&["alton-towers"]), SLOW);
    assert_eq!(planned(&jobs), [("alton-towers", true, true), ("thorpe-park", true, true)]);
}

#[test]
fn parks_nobody_wants_any_more_are_forgotten() {
    let store = Store::new();
    store.want(&slugs(&["thorpe-park"]));
    store.lock().entries.get_mut("thorpe-park").unwrap().wanted_until =
        Some(Instant::now() - Duration::from_secs(1));
    let (jobs, _) = store.plan(&slugs(&["alton-towers"]), SLOW);
    assert_eq!(planned(&jobs), [("alton-towers", true, true)]);
    assert!(!store.lock().entries.contains_key("thorpe-park"));
}

#[test]
fn a_configured_park_is_never_forgotten() {
    let store = Store::new();
    let configured = slugs(&["alton-towers"]);
    store.plan(&configured, SLOW);
    store.lock().entries.get_mut("alton-towers").unwrap().wanted_until =
        Some(Instant::now() - Duration::from_secs(1));
    let (jobs, _) = store.plan(&configured, SLOW);
    assert_eq!(jobs.len(), 1, "wanted_until is renewed while the park stays configured");
}

// ---- recording --------------------------------------------------------------

fn view(store: &Store, slug: &str) -> ParkView {
    store.view(slug, &Filter::all()).unwrap()
}

#[test]
fn recording_a_fetch_stores_it_and_bumps_the_generation() {
    let store = Store::new();
    store.plan(&slugs(&["alton-towers"]), SLOW);
    let before = store.generation();
    store.record("alton-towers", Some(Ok(vec![ride("Nemesis", true, 20)])), Some(Ok(weather(14.5))));
    let v = view(&store, "alton-towers");
    assert_eq!(v.rides.len(), 1);
    assert!(v.fetched_at.is_some() && v.error.is_none());
    assert_eq!(v.weather.unwrap().temp_c, 14.5);
    assert!(store.generation() > before);
}

#[test]
fn a_failed_fetch_keeps_the_last_good_rides_and_reports_the_error() {
    let store = Store::new();
    store.plan(&slugs(&["alton-towers"]), SLOW);
    store.record("alton-towers", Some(Ok(vec![ride("Nemesis", true, 20)])), None);
    store.record("alton-towers", Some(Err("timeout".into())), None);
    let v = view(&store, "alton-towers");
    assert_eq!(v.rides.len(), 1, "still showing the old data");
    assert_eq!(v.error.as_deref(), Some("timeout"));
    store.record("alton-towers", Some(Ok(vec![])), None);
    assert_eq!(view(&store, "alton-towers").error, None, "a good fetch clears the error");
}

#[test]
fn a_failed_weather_poll_keeps_the_previous_reading() {
    let store = Store::new();
    store.plan(&slugs(&["alton-towers"]), SLOW);
    store.record("alton-towers", None, Some(Ok(weather(14.5))));
    store.record("alton-towers", None, Some(Err("offline".into())));
    assert_eq!(view(&store, "alton-towers").weather.unwrap().temp_c, 14.5);
}

#[test]
fn recording_for_a_forgotten_park_is_ignored() {
    let store = Store::new();
    let before = store.generation();
    store.record("alton-towers", Some(Ok(vec![ride("Nemesis", true, 20)])), None);
    assert_eq!(store.generation(), before);
    assert!(view(&store, "alton-towers").rides.is_empty());
}

// ---- views ------------------------------------------------------------------

#[test]
fn views_are_filtered_and_unknown_parks_have_none() {
    let store = Store::new();
    store.plan(&slugs(&["alton-towers"]), SLOW);
    let oblivion = Ride { land: Some("Thrills".into()), ..ride("Oblivion", true, 5) };
    store.record("alton-towers", Some(Ok(vec![oblivion, ride("Monorail", true, 2)])), None);
    let groups = Groups::builtin();
    let thrill = groups.compile(&["thrill".into()]).unwrap();
    let names: Vec<_> =
        store.view("alton-towers", &thrill).unwrap().rides.into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["Oblivion"]);
    assert!(store.view("nowhere", &Filter::all()).is_none());
}

#[test]
fn a_known_park_with_no_data_yet_has_an_empty_view() {
    let v = view(&Store::new(), "thorpe-park");
    assert_eq!((v.name.as_str(), v.rides.len(), v.fetched_at), ("Thorpe Park", 0, None));
}

#[test]
fn park_views_serialise_with_the_slug_as_id() {
    let json = serde_json::to_value(view(&Store::new(), "thorpe-park")).unwrap();
    assert_eq!(json["id"], "thorpe-park");
    assert!(json.get("slug").is_none());
}

// ---- live settings ----------------------------------------------------------

#[test]
fn update_applies_a_patch_and_signals_displays() {
    let s = shared();
    let before = s.settings_gen();
    let next = s.update(Patch { dwell: Some(20), parks: Some(slugs(&["thorpe-park"])), ..Patch::default() });
    assert_eq!(next.unwrap().dwell, 20);
    assert_eq!(s.settings().parks, ["thorpe-park"]);
    assert_eq!(s.settings_gen(), before + 1);
}

#[test]
fn update_rejects_unknown_parks_and_groups_without_changing_anything() {
    let s = shared();
    let before = s.settings();
    let gen = s.settings_gen();
    let bad_park = s.update(Patch { dwell: Some(20), parks: Some(slugs(&["nowhere"])), ..Patch::default() });
    assert!(bad_park.unwrap_err().contains("unknown park"));
    let bad_group = s.update(Patch { dwell: Some(20), show: Some(slugs(&["nope"])), ..Patch::default() });
    assert!(bad_group.unwrap_err().contains("unknown group"));
    assert_eq!(s.settings(), before);
    assert_eq!(s.settings_gen(), gen, "displays aren't told to redraw for a change that didn't happen");
}

#[test]
fn update_accepts_custom_groups() {
    let s = shared();
    let custom = toml::from_str::<GroupDef>("rides = [\"Nemesis\"]").unwrap();
    s.groups.write().unwrap().add_custom([("mine".to_string(), custom)].into()).unwrap();
    assert!(s.update(Patch { show: Some(slugs(&["mine"])), ..Patch::default() }).is_ok());
}
