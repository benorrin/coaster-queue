use serde::{Deserialize, Serialize};
use std::{sync::OnceLock, time::Duration};

/// A park on queue-times.com.
#[derive(Clone)]
pub struct Park {
    /// URL-safe name, e.g. "alton-towers"; how parks are referred to on the command line and web.
    pub slug: String,
    pub name: String,
    /// queue-times.com park id.
    pub id: u32,
    pub lat: f64,
    pub lon: f64,
}

/// Used when the park directory can't be fetched: name, queue-times id, latitude, longitude.
const BUILT_IN: [(&str, u32, f64, f64); 2] = [
    ("Alton Towers", 1, 52.9871, -1.8872),
    ("Thorpe Park", 2, 51.4034, -0.5130),
];

/// The two parks shown when none are asked for.
pub fn default_parks() -> Vec<Park> {
    BUILT_IN
        .iter()
        .map(|&(name, id, lat, lon)| Park { slug: slugify(name), name: name.into(), id, lat, lon })
        .collect()
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Countries queue-times uses for the UK's nations.
const UK_COUNTRIES: [&str; 5] = ["England", "Scotland", "Wales", "Northern Ireland", "United Kingdom"];

#[derive(Deserialize)]
struct Group {
    parks: Vec<DirectoryPark>,
}

#[derive(Deserialize)]
struct DirectoryPark {
    id: u32,
    name: String,
    country: String,
    latitude: String,
    longitude: String,
}

/// Every UK park queue-times knows about, sorted by name.
pub fn fetch_parks() -> Result<Vec<Park>, String> {
    let groups: Vec<Group> = agent()
        .get("https://queue-times.com/parks.json")
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let mut parks: Vec<Park> = groups
        .into_iter()
        .flat_map(|g| g.parks)
        .filter(|p| UK_COUNTRIES.contains(&p.country.as_str()))
        .map(|p| Park {
            slug: slugify(&p.name),
            lat: p.latitude.parse().unwrap_or(0.0),
            lon: p.longitude.parse().unwrap_or(0.0),
            name: p.name,
            id: p.id,
        })
        .collect();
    parks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(parks)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Ride {
    pub name: String,
    pub is_open: bool,
    pub wait_time: u32,
    /// The area of the park the ride is in, when queue-times files it under one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub land: Option<String>,
}

#[derive(Deserialize)]
struct Land {
    name: String,
    rides: Vec<Ride>,
}

/// One agent for every request, so connections (and their TLS sessions) are reused
/// instead of renegotiated each poll, which matters on a small board.
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build())
}

#[derive(Deserialize)]
struct Response {
    lands: Vec<Land>,
    rides: Vec<Ride>,
}

pub fn fetch(id: u32) -> Result<Vec<Ride>, String> {
    let url = format!("https://queue-times.com/parks/{id}/queue_times.json");
    let resp: Response = agent()
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let mut rides = resp.rides;
    for land in resp.lands {
        let name = land.name;
        rides.extend(land.rides.into_iter().map(|mut r| {
            r.land = Some(name.clone());
            r
        }));
    }
    rides.sort_by(|a, b| {
        b.is_open
            .cmp(&a.is_open)
            .then(b.wait_time.cmp(&a.wait_time))
            .then(a.name.cmp(&b.name))
    });
    Ok(rides)
}

// ---- weather -----------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
pub struct Weather {
    #[serde(alias = "temperature_2m")]
    pub temp_c: f64,
    #[serde(alias = "apparent_temperature")]
    pub feels_c: f64,
    /// WMO weather interpretation code.
    #[serde(alias = "weather_code")]
    pub code: u32,
    #[serde(alias = "wind_speed_10m")]
    pub wind_mph: f64,
    #[serde(alias = "precipitation")]
    pub rain_mm: f64,
}

#[derive(Deserialize)]
struct WeatherResponse {
    current: Weather,
}

/// Current conditions from Open-Meteo (free, no key; CC BY 4.0).
pub fn fetch_weather(lat: f64, lon: f64) -> Result<Weather, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m,precipitation\
         &wind_speed_unit=mph&timezone=Europe%2FLondon"
    );
    let resp: WeatherResponse = agent()
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    Ok(resp.current)
}
