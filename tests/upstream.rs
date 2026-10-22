//! Decoding of the upstream services' responses, against saved copies of their JSON in
//! `tests/fixtures/`. If queue-times or Open-Meteo change their format these are what
//! should fail, rather than the display going blank. No network is used.

use coaster_queue::queue::{
    default_parks, find_park, parse_parks, parse_queue_times, parse_weather, slugify,
};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn parks_keeps_only_uk_parks_sorted_by_name() {
    let parks = parse_parks(&fixture("parks.json")).unwrap();
    let slugs: Vec<&str> = parks.iter().map(|p| p.slug.as_str()).collect();
    assert_eq!(slugs, ["alton-towers", "blackpool-pleasure-beach", "mystery-park", "thorpe-park"]);
}

#[test]
fn parks_carry_id_and_coordinates() {
    let parks = parse_parks(&fixture("parks.json")).unwrap();
    let alton = find_park(&parks, "alton-towers").unwrap();
    assert_eq!((alton.id, alton.name.as_str()), (1, "Alton Towers"));
    assert_eq!((alton.lat, alton.lon), (52.9871, -1.8872));
}

/// Regression: a park whose coordinates don't parse is still listed (at 0,0) rather than
/// failing the whole directory.
#[test]
fn parks_with_bad_coordinates_are_kept() {
    let parks = parse_parks(&fixture("parks.json")).unwrap();
    let mystery = find_park(&parks, "mystery").unwrap();
    assert_eq!((mystery.lat, mystery.lon), (0.0, 0.0));
}

#[test]
fn parks_rejects_garbage() {
    assert!(parse_parks("not json").is_err());
    assert!(parse_parks(r#"{"parks": []}"#).is_err(), "must be a list of companies");
}

#[test]
fn queue_times_merges_lands_and_unfiled_rides() {
    let rides = parse_queue_times(&fixture("queue_times.json")).unwrap();
    assert_eq!(rides.len(), 5);
    let land = |name: &str| rides.iter().find(|r| r.name == name).unwrap().land.as_deref();
    assert_eq!(land("Nemesis"), Some("Thrills"));
    assert_eq!(land("Carousel"), Some("Family"));
    assert_eq!(land("Monorail"), None, "rides outside any land have no land");
}

/// Open rides come first, longest wait first, ties by name; closed rides go last.
#[test]
fn queue_times_are_sorted() {
    let rides = parse_queue_times(&fixture("queue_times.json")).unwrap();
    let names: Vec<&str> = rides.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Monorail", "Aardvark Express", "Nemesis", "Carousel", "Oblivion"]);
}

#[test]
fn queue_times_rejects_garbage() {
    assert!(parse_queue_times("").is_err());
    assert!(parse_queue_times(r#"{"lands": []}"#).is_err(), "`rides` is required");
}

#[test]
fn weather_decodes_open_meteo_field_names() {
    let w = parse_weather(&fixture("weather.json")).unwrap();
    assert_eq!((w.temp_c, w.feels_c, w.code), (18.5, 16.2, 61));
    assert_eq!((w.wind_mph, w.rain_mm), (12.3, 0.4));
}

#[test]
fn weather_rejects_missing_current_conditions() {
    assert!(parse_weather(r#"{"latitude": 1.0}"#).is_err());
}

#[test]
fn slugs() {
    assert_eq!(slugify("Alton Towers"), "alton-towers");
    assert_eq!(slugify("  Chessington World of Adventures! "), "chessington-world-of-adventures");
    assert_eq!(slugify("Drayton   Manor -- Park"), "drayton-manor-park");
    assert_eq!(slugify("!!!"), "");
}

#[test]
fn park_lookup_by_exact_name_or_unique_prefix() {
    let parks = parse_parks(&fixture("parks.json")).unwrap();
    assert_eq!(find_park(&parks, "Thorpe-Park").unwrap().id, 2, "case-insensitive");
    assert_eq!(find_park(&parks, "thor").unwrap().id, 2);
    assert!(find_park(&parks, "nowhere").unwrap_err().contains("unknown park"));
    let both = [default_parks(), parks].concat();
    assert!(find_park(&both, "a").unwrap_err().contains("several parks"), "ambiguous prefix");
}

/// Regression: an exact slug wins even when it is also the prefix of a longer one.
#[test]
fn exact_park_name_beats_longer_prefix_match() {
    let mut parks = default_parks();
    let mut longer = parks[0].clone();
    longer.slug = "alton-towers-resort".into();
    parks.push(longer);
    assert_eq!(find_park(&parks, "alton-towers").unwrap().slug, "alton-towers");
}
