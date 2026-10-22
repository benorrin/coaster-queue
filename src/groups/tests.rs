use super::*;

fn ride(name: &str, land: Option<&str>) -> Ride {
    Ride { name: name.into(), is_open: true, wait_time: 10, land: land.map(Into::into) }
}

#[test]
fn built_in_groups_parse_and_match() {
    let g = Groups::builtin();
    let thrill = g.compile(&["thrill".into()]).unwrap();
    assert!(thrill.allows("alton-towers", &ride("Oblivion", Some("Thrills"))));
    assert!(thrill.allows("thorpe-park", &ride("Stealth", Some("Coasters"))));
    assert!(thrill.allows("chessington-world-of-adventures", &ride("Vampire", None)));
    assert!(!thrill.allows("alton-towers", &ride("The Royal Carousel", Some("Family"))));
    let transport = g.compile(&["transport".into()]).unwrap();
    assert!(transport.allows("alton-towers", &ride("Monorail to Entrance Plaza", Some("Family"))));
    assert!(transport.allows("flamingoland", &ride("People Mover", Some("Transport"))));
    let water = g.compile(&["water".into()]).unwrap();
    assert!(water.allows("alton-towers", &ride("Congo River Rapids", None)));
}

#[test]
fn empty_show_allows_everything_and_unknown_groups_fail() {
    let g = Groups::builtin();
    assert!(g.compile(&[]).unwrap().allows("x", &ride("Anything", None)));
    assert!(g.compile(&["nope".into()]).is_err());
}

#[test]
fn custom_groups_include_and_restrict() {
    let mut g = Groups::builtin();
    let custom: BTreeMap<String, GroupDef> = toml::from_str::<File>(
        r#"
        [groups.favs]
        rides = ["Nemesis*", "The Smiler"]
        parks = ["alton-towers"]
        [groups.big-day]
        include = ["favs", "transport"]
        [groups.loop-a]
        include = ["loop-b"]
        [groups.loop-b]
        include = ["loop-a"]
        "#,
    )
    .unwrap()
    .groups;
    g.add_custom(custom).unwrap();
    let favs = g.compile(&["favs".into()]).unwrap();
    assert!(favs.allows("alton-towers", &ride("Nemesis Reborn", None)));
    assert!(!favs.allows("thorpe-park", &ride("Nemesis Inferno", None)), "limited to Alton Towers");
    let day = g.compile(&["big-day".into()]).unwrap();
    assert!(day.allows("alton-towers", &ride("The Smiler", None)));
    assert!(day.allows("alton-towers", &ride("Skyride to Towers St.", None)));
    assert!(g.compile(&["loop-a".into()]).is_ok(), "include cycles terminate");
}

#[test]
fn glob_and_normalise() {
    assert!(glob("*rapids*", "alton mountain rapids"));
    assert!(glob("nemesis*", "nemesis inferno"));
    assert!(!glob("nemesis", "nemesis inferno"));
    assert_eq!(normalise("LEGO® NINJAGO® The Ride"), "lego ninjago the ride");
    assert_eq!(normalise("Andy’s Adventures"), "andy's adventures");
}
