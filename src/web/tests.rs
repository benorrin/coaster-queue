use super::*;

/// Every file in `web/` must be in `ASSETS`, or the page would silently break.
#[test]
fn every_web_file_is_served() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/web");
    for entry in fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let path = if name == "index.html" { "/".to_string() } else { format!("/{name}") };
        assert!(ASSETS.iter().any(|a| a.0 == path), "{name} is not listed in ASSETS");
    }
}

#[test]
fn query_parsing() {
    assert_eq!(query_param("parks=a,b&show=x", "show"), Some("x"));
    assert_eq!(query_param("parks=a", "show"), None);
    assert_eq!(query_param("show=", "show"), Some(""));
    assert_eq!(list("a%2Cb, c,,"), ["a", "b", "c"]);
}
