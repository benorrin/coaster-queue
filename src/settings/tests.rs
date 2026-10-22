use super::*;

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn applies_pairs() {
    let mut s = Settings::default();
    let p = Patch::from_pairs(&pairs(&[
        ("dwell", "12"),
        ("parks", "thorpe-park, alton-towers"),
        ("show_closed", "on"),
        ("theme", "amber"),
        ("feels_like", "off"),
    ]))
    .unwrap();
    s.apply(p).unwrap();
    assert_eq!((s.dwell, s.show_closed, s.theme.as_str(), s.feels_like), (12, true, "amber", false));
    assert_eq!(s.parks, ["thorpe-park", "alton-towers"]);
}

#[test]
fn rejects_bad_values_without_partial_changes() {
    let mut s = Settings::default();
    let before = s.clone();
    let p = Patch { dwell: Some(10), theme: Some("neon".into()), ..Patch::default() };
    assert!(s.apply(p).is_err());
    assert_eq!(s, before);
    assert!(s.apply(Patch { parks: Some(vec![]), ..Patch::default() }).is_err());
    assert!(s.apply(Patch { brightness: Some(2.0), ..Patch::default() }).is_err());
    assert!(Patch::from_pairs(&pairs(&[("nope", "1")])).is_err());
    assert!(Patch::from_pairs(&pairs(&[("flip", "maybe")])).is_err());
}

#[test]
fn dedupes_lists() {
    let mut s = Settings::default();
    s.apply(Patch {
        show: Some(vec!["thrill".into(), "thrill".into(), "family".into()]),
        ..Patch::default()
    })
    .unwrap();
    assert_eq!(s.show, ["thrill", "family"]);
}
