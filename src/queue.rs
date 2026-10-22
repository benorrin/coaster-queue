use serde::Deserialize;
use std::time::Duration;

pub const PARKS: [(&str, &str, u32); 2] = [
    ("alton", "Alton Towers", 1),
    ("thorpe", "Thorpe Park", 2),
];

#[derive(Deserialize)]
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
