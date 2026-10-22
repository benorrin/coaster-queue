use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const PARKS: [(&str, &str, u32); 2] = [
    ("alton", "Alton Towers", 1),
    ("thorpe", "Thorpe Park", 2),
];

#[derive(Clone, Serialize, Deserialize)]
pub struct Ride {
    pub name: String,
    pub is_open: bool,
    pub wait_time: u32,
}

#[derive(Deserialize)]
struct Land {
    rides: Vec<Ride>,
}

#[derive(Deserialize)]
struct Response {
    lands: Vec<Land>,
    rides: Vec<Ride>,
}

pub fn fetch(id: u32) -> Result<Vec<Ride>, String> {
    let url = format!("https://queue-times.com/parks/{id}/queue_times.json");
    let resp: Response = ureq::get(&url)
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let mut rides = resp.rides;
    rides.extend(resp.lands.into_iter().flat_map(|l| l.rides));
    rides.sort_by(|a, b| {
        b.is_open
            .cmp(&a.is_open)
            .then(b.wait_time.cmp(&a.wait_time))
            .then(a.name.cmp(&b.name))
    });
    Ok(rides)
}

// ---- weather -----------------------------------------------------------

/// Where each park is, for the weather lookup.
pub fn location(slug: &str) -> (f64, f64) {
    match slug {
        "alton" => (52.9871, -1.8872),
        _ => (51.4034, -0.5130), // thorpe
    }
}

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
pub fn fetch_weather(slug: &str) -> Result<Weather, String> {
    let (lat, lon) = location(slug);
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m,precipitation\
         &wind_speed_unit=mph&timezone=Europe%2FLondon"
    );
    let resp: WeatherResponse = ureq::get(&url)
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    Ok(resp.current)
}
