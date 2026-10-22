//! The settings every front end shares. They come from built-in defaults, then the
//! config file, then command-line flags, and can be changed while running through
//! the API; all of those go through `Settings::apply` so the rules live in one place.

use serde::{Deserialize, Serialize};

pub const THEMES: [&str; 5] = ["rgb", "amber", "green", "ice", "rainbow"];
pub const MAX_PARKS: usize = 12;
const DEFAULT_PARKS: [&str; 2] = ["alton-towers", "thorpe-park"];

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Settings {
    /// Slugs of the parks to show, e.g. "alton-towers".
    pub parks: Vec<String>,
    /// Ride groups to restrict the display to (built-in or custom); empty shows every ride.
    pub show: Vec<String>,
    /// Seconds each page of rides stays up.
    pub dwell: u32,
    pub show_closed: bool,
    /// Seconds between queue-time polls.
    pub interval: u64,
    /// 0.1 to 1.0.
    pub brightness: f32,
    pub theme: String,
    pub gradients: bool,
    pub weather: bool,
    /// Include the "feels like" temperature in the weather line.
    pub feels_like: bool,
    pub clock24: bool,
    pub flip: bool,
    pub shine: bool,
    /// Dots across the web display.
    pub cols: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            parks: DEFAULT_PARKS.iter().map(|s| s.to_string()).collect(),
            show: Vec::new(),
            dwell: 8,
            show_closed: false,
            interval: 60,
            brightness: 1.0,
            theme: "rgb".into(),
            gradients: true,
            weather: true,
            feels_like: true,
            clock24: true,
            flip: true,
            shine: true,
            cols: 200,
        }
    }
}

/// A partial change to the settings: whatever is present is applied.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub parks: Option<Vec<String>>,
    pub show: Option<Vec<String>>,
    pub dwell: Option<u32>,
    pub show_closed: Option<bool>,
    pub interval: Option<u64>,
    pub brightness: Option<f32>,
    pub theme: Option<String>,
    pub gradients: Option<bool>,
    pub weather: Option<bool>,
    pub feels_like: Option<bool>,
    pub clock24: Option<bool>,
    pub flip: Option<bool>,
    pub shine: Option<bool>,
    pub cols: Option<u32>,
}

const LIST_KEYS: [&str; 2] = ["parks", "show"];
const BOOL_KEYS: [&str; 7] = ["show_closed", "gradients", "weather", "feels_like", "clock24", "flip", "shine"];

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

impl Patch {
    /// Build a patch from `key=value` pairs as given on the command line. Lists are
    /// comma separated; booleans accept true/false, on/off, yes/no and 1/0.
    pub fn from_pairs(pairs: &[(String, String)]) -> Result<Patch, String> {
        let mut map = serde_json::Map::new();
        for (key, raw) in pairs {
            let raw = raw.trim();
            let value = if LIST_KEYS.contains(&key.as_str()) {
                serde_json::Value::Array(
                    raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(serde_json::Value::from).collect(),
                )
            } else if BOOL_KEYS.contains(&key.as_str()) {
                parse_bool(raw)
                    .map(serde_json::Value::Bool)
                    .ok_or_else(|| format!("{key} needs true or false, not {raw:?}"))?
            } else {
                serde_json::from_str(raw).unwrap_or_else(|_| serde_json::Value::String(raw.into()))
            };
            map.insert(key.clone(), value);
        }
        serde_json::from_value(serde_json::Value::Object(map)).map_err(|e| e.to_string())
    }
}

/// Group and park names are lowercase words joined by hyphens or underscores.
pub fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn ids(what: &str, list: Vec<String>, max: usize) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for id in list {
        if !valid_id(&id) {
            return Err(format!("{what}: {id:?} isn't a valid name (lowercase letters, digits, - and _)"));
        }
        if !out.contains(&id) {
            out.push(id);
        }
    }
    if out.len() > max {
        return Err(format!("{what}: at most {max} allowed"));
    }
    Ok(out)
}

fn within<T: PartialOrd + std::fmt::Display>(what: &str, v: T, lo: T, hi: T) -> Result<T, String> {
    if v < lo || v > hi {
        Err(format!("{what} must be between {lo} and {hi}"))
    } else {
        Ok(v)
    }
}

impl Settings {
    /// Apply a patch. Nothing changes unless every part of it is valid.
    pub fn apply(&mut self, p: Patch) -> Result<(), String> {
        let mut next = self.clone();
        if let Some(v) = p.parks {
            next.parks = ids("parks", v, MAX_PARKS)?;
            if next.parks.is_empty() {
                return Err("parks: choose at least one".into());
            }
        }
        if let Some(v) = p.show {
            next.show = ids("show", v, 32)?;
        }
        if let Some(v) = p.dwell {
            next.dwell = within("dwell", v, 2, 300)?;
        }
        if let Some(v) = p.interval {
            next.interval = within("interval", v, 10, 3600)?;
        }
        if let Some(v) = p.brightness {
            next.brightness = within("brightness", v, 0.1, 1.0)?;
        }
        if let Some(v) = p.cols {
            next.cols = within("cols", v, 64, 400)?;
        }
        if let Some(v) = p.theme {
            if !THEMES.contains(&v.as_str()) {
                return Err(format!("theme must be one of {}", THEMES.join(", ")));
            }
            next.theme = v;
        }
        next.show_closed = p.show_closed.unwrap_or(next.show_closed);
        next.gradients = p.gradients.unwrap_or(next.gradients);
        next.weather = p.weather.unwrap_or(next.weather);
        next.feels_like = p.feels_like.unwrap_or(next.feels_like);
        next.clock24 = p.clock24.unwrap_or(next.clock24);
        next.flip = p.flip.unwrap_or(next.flip);
        next.shine = p.shine.unwrap_or(next.shine);
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn applies_pairs() {
        let mut s = Settings::default();
        let p = Patch::from_pairs(&pairs(&[("dwell", "12"), ("parks", "thorpe-park, alton-towers"), ("show_closed", "on"), ("theme", "amber"), ("feels_like", "off")])).unwrap();
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
        s.apply(Patch { show: Some(vec!["thrill".into(), "thrill".into(), "family".into()]), ..Patch::default() }).unwrap();
        assert_eq!(s.show, ["thrill", "family"]);
    }
}
