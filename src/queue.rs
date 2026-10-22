//! Clients for the two upstream services, queue-times.com (ride waits and the park
//! list) and Open-Meteo (weather), plus the small `Park` and `Ride` types they feed.

use serde::{Deserialize, Serialize};
use std::{sync::OnceLock, time::Duration};

// ---- HTTP -------------------------------------------------------------------

/// One agent for every request, so connections (and their TLS sessions) are reused
/// instead of renegotiated each poll, which matters on a small board.
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build())
}

/// GET `url` and return the body, flattening any failure to a message.
fn get_body(url: &str) -> Result<String, String> {
    agent().get(url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())
}

// ---- parks ------------------------------------------------------------------

/// A park on queue-times.com.
#[derive(Clone, Debug)]
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
const BUILT_IN: [(&str, u32, f64, f64); 2] =
    [("Alton Towers", 1, 52.9871, -1.8872), ("Thorpe Park", 2, 51.4034, -0.5130)];

/// Countries queue-times uses for the UK's nations.
const UK_COUNTRIES: [&str; 5] = ["England", "Scotland", "Wales", "Northern Ireland", "United Kingdom"];

/// The two parks that are always available, and shown when none are asked for.
pub fn default_parks() -> Vec<Park> {
    BUILT_IN
        .iter()
        .map(|&(name, id, lat, lon)| Park { slug: slugify(name), name: name.into(), id, lat, lon })
        .collect()
}

/// "Alton Towers" becomes "alton-towers".
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Every UK park queue-times knows about, sorted by name.
pub fn fetch_parks() -> Result<Vec<Park>, String> {
    parse_parks(&get_body("https://queue-times.com/parks.json")?)
}

/// Decode queue-times' park list, which is grouped by company; that grouping is flattened
/// away and only UK parks are kept.
pub fn parse_parks(body: &str) -> Result<Vec<Park>, String> {
    #[derive(Deserialize)]
    struct Company {
        parks: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        id: u32,
        name: String,
        country: String,
        latitude: String,
        longitude: String,
    }

    let companies: Vec<Company> = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut parks: Vec<Park> = companies
        .into_iter()
        .flat_map(|c| c.parks)
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

/// The park directory, or just the built-in parks (with a warning) if it can't be fetched.
pub fn directory_or_default() -> Vec<Park> {
    fetch_parks().unwrap_or_else(|e| {
        eprintln!(
            "warning: couldn't load the park list ({e}); only Alton Towers and Thorpe Park are available"
        );
        default_parks()
    })
}

/// Resolve a name given on the command line against park slugs: exactly, or as the
/// start of exactly one.
pub fn find_park(directory: &[Park], name: &str) -> Result<Park, String> {
    let name = name.to_lowercase();
    if let Some(p) = directory.iter().find(|p| p.slug == name) {
        return Ok(p.clone());
    }
    let matches: Vec<&Park> = directory.iter().filter(|p| p.slug.starts_with(&name)).collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!("unknown park: {name} (see --list-parks)")),
        many => {
            let slugs: Vec<&str> = many.iter().map(|p| p.slug.as_str()).collect();
            Err(format!("{name} matches several parks: {}", slugs.join(", ")))
        }
    }
}

// ---- queue times ------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ride {
    pub name: String,
    pub is_open: bool,
    pub wait_time: u32,
    /// The area of the park the ride is in, when queue-times files it under one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub land: Option<String>,
}

/// Current waits for every ride in a park: open rides first, longest wait first.
pub fn fetch(park_id: u32) -> Result<Vec<Ride>, String> {
    parse_queue_times(&get_body(&format!("https://queue-times.com/parks/{park_id}/queue_times.json"))?)
}

/// Decode a park's queue-times response into one list of rides, each tagged with its land.
pub fn parse_queue_times(body: &str) -> Result<Vec<Ride>, String> {
    #[derive(Deserialize)]
    struct Land {
        name: String,
        rides: Vec<Ride>,
    }
    #[derive(Deserialize)]
    struct Response {
        lands: Vec<Land>,
        /// Rides queue-times hasn't filed under a land.
        rides: Vec<Ride>,
    }

    let resp: Response = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut rides = resp.rides;
    for land in resp.lands {
        let name = land.name;
        rides.extend(land.rides.into_iter().map(|r| Ride { land: Some(name.clone()), ..r }));
    }
    rides.sort_by(|a, b| {
        b.is_open.cmp(&a.is_open).then(b.wait_time.cmp(&a.wait_time)).then_with(|| a.name.cmp(&b.name))
    });
    Ok(rides)
}

// ---- weather ----------------------------------------------------------------

/// Current conditions. The aliases are Open-Meteo's field names, so this decodes their
/// response directly; the API serves it under the clearer names.
#[derive(Clone, Debug, Serialize, Deserialize)]
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

/// Current conditions from Open-Meteo (free, no key; CC BY 4.0).
pub fn fetch_weather(lat: f64, lon: f64) -> Result<Weather, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m,precipitation\
         &wind_speed_unit=mph&timezone=Europe%2FLondon"
    );
    parse_weather(&get_body(&url)?)
}

/// Decode an Open-Meteo forecast response into its current conditions.
pub fn parse_weather(body: &str) -> Result<Weather, String> {
    #[derive(Deserialize)]
    struct Response {
        current: Weather,
    }

    Ok(serde_json::from_str::<Response>(body).map_err(|e| e.to_string())?.current)
}
