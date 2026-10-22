//! Ride groups: named sets of rides to show, from the built-in categories or custom
//! ones in the config file. A group can match rides by name, by land, include other
//! groups, and be limited to some parks. Custom lists of rides are just groups.

use crate::queue::Ride;
use crate::settings::valid_id;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GroupDef {
    pub label: Option<String>,
    /// Other groups whose rides are included.
    pub include: Vec<String>,
    /// Ride name patterns.
    pub rides: Vec<String>,
    /// Land name patterns.
    pub lands: Vec<String>,
    /// Park slugs the group applies to; empty means every park.
    pub parks: Vec<String>,
}

#[derive(Deserialize)]
struct File {
    groups: BTreeMap<String, GroupDef>,
}

pub struct GroupInfo<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub builtin: bool,
}

struct Entry {
    def: GroupDef,
    builtin: bool,
}

pub struct Groups {
    defs: BTreeMap<String, Entry>,
}

impl Groups {
    pub fn builtin() -> Groups {
        let file: File = toml::from_str(include_str!("groups.toml")).expect("built-in groups.toml is valid");
        Groups { defs: file.groups.into_iter().map(|(k, def)| (k, Entry { def, builtin: true })).collect() }
    }

    /// Add custom groups; one with a built-in's name replaces it.
    pub fn add_custom(&mut self, custom: BTreeMap<String, GroupDef>) -> Result<(), String> {
        for (id, def) in custom {
            if !valid_id(&id) {
                return Err(format!("group name {id:?} isn't valid (lowercase letters, digits, - and _)"));
            }
            self.defs.insert(id, Entry { def, builtin: false });
        }
        Ok(())
    }

    pub fn list(&self) -> Vec<GroupInfo<'_>> {
        self.defs
            .iter()
            .map(|(id, e)| GroupInfo { id, label: e.def.label.as_deref().unwrap_or(id), builtin: e.builtin })
            .collect()
    }

    pub fn def(&self, id: &str) -> Option<&GroupDef> {
        self.defs.get(id).map(|e| &e.def)
    }

    /// A filter that passes rides belonging to any of the named groups; no names passes everything.
    pub fn compile(&self, show: &[String]) -> Result<Filter, String> {
        if show.is_empty() {
            return Ok(Filter { parts: None });
        }
        let mut parts = Vec::new();
        let mut seen = Vec::new();
        for id in show {
            self.expand(id, &mut seen, &mut parts)?;
        }
        Ok(Filter { parts: Some(parts) })
    }

    fn expand(&self, id: &str, seen: &mut Vec<String>, out: &mut Vec<Part>) -> Result<(), String> {
        let def = self.def(id).ok_or_else(|| {
            let known: Vec<&str> = self.defs.keys().map(String::as_str).collect();
            format!("unknown group {id:?} (known: {})", known.join(", "))
        })?;
        if seen.iter().any(|s| s == id) {
            return Ok(()); // already included; also stops include cycles
        }
        seen.push(id.to_string());
        out.push(Part {
            parks: def.parks.clone(),
            rides: def.rides.iter().map(|p| normalise(p)).collect(),
            lands: def.lands.iter().map(|p| normalise(p)).collect(),
        });
        for inc in &def.include {
            self.expand(inc, seen, out)?;
        }
        Ok(())
    }
}

struct Part {
    parks: Vec<String>,
    rides: Vec<String>,
    lands: Vec<String>,
}

pub struct Filter {
    /// None passes every ride.
    parts: Option<Vec<Part>>,
}

impl Filter {
    pub fn all() -> Filter {
        Filter { parts: None }
    }

    pub fn allows(&self, park: &str, ride: &Ride) -> bool {
        let Some(parts) = &self.parts else { return true };
        let name = normalise(&ride.name);
        let land = ride.land.as_deref().map(normalise);
        parts.iter().any(|p| {
            (p.parks.is_empty() || p.parks.iter().any(|s| s == park))
                && (p.rides.iter().any(|pat| glob(pat, &name))
                    || land.as_deref().is_some_and(|l| p.lands.iter().any(|pat| glob(pat, l))))
        })
    }
}

/// Lowercase, straighten quotes and drop trademark marks so names compare reliably.
pub fn normalise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            '’' | '‘' => out.push('\''),
            '®' | '©' | '™' => {}
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

/// Match `text` against a pattern where `*` matches any run of characters.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
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
}
