use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn run_components() {
    let r = Run::parse(&args(&["led", "web"])).unwrap();
    assert!(r.led && r.web && r.api, "web implies api");
    assert!(Run::parse(&args(&["all"])).unwrap().led);
    assert!(Run::parse(&args(&["tv"])).is_err());
}

#[test]
fn file_then_flags() {
    let mut cfg = Config::default();
    let mut run = None;
    apply_file(
        &mut cfg,
        r#"
        run = ["api"]
        parks = ["thorpe-park"]
        dwell = 5
        [server]
        port = 9000
        [groups.favs]
        rides = ["Stealth"]
        "#,
        &mut run,
    )
    .unwrap();
    assert_eq!(run.unwrap(), ["api"]);
    assert_eq!((cfg.settings.dwell, cfg.server.port), (5, 9000));
    assert_eq!(cfg.settings.parks, ["thorpe-park"]);
    assert!(cfg.custom_groups.contains_key("favs"));
    assert!(apply_file(&mut cfg, "dwel = 5", &mut None).is_err(), "typos are errors");
    assert!(apply_file(&mut cfg, "dwell = 1", &mut None).is_err(), "values are validated");
}

#[test]
fn parses_flags() {
    let a = parse_args(args(&["-r", "led,api", "-a", "-s", "dwell=9", "chess", "-S", "thrill"])).unwrap();
    assert_eq!(a.run.unwrap(), ["led", "api"]);
    assert_eq!(a.names, ["chess"]);
    assert_eq!(a.sets.len(), 3);
    assert!(parse_args(args(&["--nope"])).is_err());
    assert!(parse_args(args(&["-s", "noequals"])).is_err());
    assert_eq!(parse_args(args(&["serve", "-p", "9"])).unwrap().run.unwrap(), ["api", "web"]);
}
