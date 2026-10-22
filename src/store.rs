//! Data shared by every component: the settings, the ride groups and the latest
//! queue times and weather. One poller thread keeps the store fresh, fetching only
//! the parks somebody currently wants (the configured parks, plus any a client has
//! asked the API for recently) so a small board makes as few requests as it can.

use crate::groups::{Filter, Groups};
use crate::queue::{self, Park, Ride, Weather};
use crate::settings::{Patch, Settings};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex, RwLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// How long a park keeps being polled after a client last asked for it.
const WANTED_FOR: Duration = Duration::from_secs(600);
const WEATHER_EVERY: Duration = Duration::from_secs(600);
const DIRECTORY_EVERY: Duration = Duration::from_secs(24 * 3600);
const DIRECTORY_RETRY: Duration = Duration::from_secs(60);

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub struct Shared {
    settings: RwLock<Settings>,
    groups: RwLock<Groups>,
    pub store: Store,
    settings_gen: AtomicU64,
    pub quit: AtomicBool,
}

impl Shared {
    pub fn new(settings: Settings, groups: Groups) -> Arc<Shared> {
        Arc::new(Shared {
            settings: RwLock::new(settings),
            groups: RwLock::new(groups),
            store: Store::new(),
            settings_gen: AtomicU64::new(0),
            quit: AtomicBool::new(false),
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// Bumped whenever the settings change, so displays know to redraw.
    pub fn settings_gen(&self) -> u64 {
        self.settings_gen.load(Ordering::Relaxed)
    }

    pub fn groups<R>(&self, f: impl FnOnce(&Groups) -> R) -> R {
        f(&self.groups.read().unwrap())
    }

    pub fn filter(&self, show: &[String]) -> Result<Filter, String> {
        self.groups(|g| g.compile(show))
    }

    /// Apply a change to the live settings, checking that the parks and groups it names exist.
    pub fn update(&self, patch: Patch) -> Result<Settings, String> {
        let mut next = self.settings();
        next.apply(patch)?;
        let directory = self.store.directory();
        if let Some(bad) = next.parks.iter().find(|p| !directory.iter().any(|(slug, _)| slug == *p)) {
            return Err(format!("unknown park {bad:?}"));
        }
        self.filter(&next.show)?;
        *self.settings.write().unwrap() = next.clone();
        self.settings_gen.fetch_add(1, Ordering::Relaxed);
        self.store.wake();
        Ok(next)
    }
}

struct Entry {
    rides: Vec<Ride>,
    fetched_at: Option<u64>,
    tried: Option<Instant>,
    error: Option<String>,
    weather: Option<Weather>,
    weather_tried: Option<Instant>,
    wanted_until: Option<Instant>,
}

impl Entry {
    fn new() -> Entry {
        Entry { rides: Vec::new(), fetched_at: None, tried: None, error: None, weather: None, weather_tried: None, wanted_until: None }
    }
}

struct Inner {
    directory: Vec<Park>,
    entries: HashMap<String, Entry>,
}

pub struct Store {
    inner: Mutex<Inner>,
    wake: Condvar,
    generation: AtomicU64,
}

/// One park's data as seen through a ride filter.
#[derive(Serialize)]
pub struct ParkView {
    #[serde(rename = "id")]
    pub slug: String,
    pub name: String,
    pub rides: Vec<Ride>,
    pub fetched_at: Option<u64>,
    pub error: Option<String>,
    pub weather: Option<Weather>,
}

impl Store {
    fn new() -> Store {
        Store {
            // Start with the built-in parks so everything works before the directory loads.
            inner: Mutex::new(Inner { directory: queue::default_parks(), entries: HashMap::new() }),
            wake: Condvar::new(),
            generation: AtomicU64::new(0),
        }
    }

    /// Bumped whenever fresh data arrives.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Every park on offer as (slug, name).
    pub fn directory(&self) -> Vec<(String, String)> {
        self.inner.lock().unwrap().directory.iter().map(|p| (p.slug.clone(), p.name.clone())).collect()
    }

    /// The park's current data with `filter` applied, or None if the park is unknown.
    pub fn view(&self, slug: &str, filter: &Filter) -> Option<ParkView> {
        let inner = self.inner.lock().unwrap();
        let park = inner.directory.iter().find(|p| p.slug == slug)?;
        let blank = Entry::new();
        let e = inner.entries.get(slug).unwrap_or(&blank);
        Some(ParkView {
            slug: park.slug.clone(),
            name: park.name.clone(),
            rides: e.rides.iter().filter(|r| filter.allows(slug, r)).cloned().collect(),
            fetched_at: e.fetched_at,
            error: e.error.clone(),
            weather: e.weather.clone(),
        })
    }

    /// Note that a client wants these parks, so they get polled for the next few minutes.
    pub fn want(&self, slugs: &[String]) {
        let mut inner = self.inner.lock().unwrap();
        let until = Instant::now() + WANTED_FOR;
        let mut new = false;
        for slug in slugs {
            if inner.directory.iter().any(|p| &p.slug == slug) {
                let e = inner.entries.entry(slug.clone()).or_insert_with(Entry::new);
                new |= e.tried.is_none();
                e.wanted_until = Some(until);
            }
        }
        drop(inner);
        if new {
            self.wake.notify_all();
        }
    }

    /// Poll everything again now.
    pub fn refresh(&self) {
        for e in self.inner.lock().unwrap().entries.values_mut() {
            e.tried = None;
        }
        self.wake.notify_all();
    }

    pub fn wake(&self) {
        self.wake.notify_all();
    }
}

struct Job {
    slug: String,
    park: Park,
    queue: bool,
    weather: bool,
}

/// Start the poller. It runs until `shared.quit` is set.
pub fn spawn_poller(shared: Arc<Shared>) {
    thread::spawn(move || poll_loop(&shared));
}

fn poll_loop(shared: &Shared) {
    let store = &shared.store;
    let mut directory_due = Instant::now();
    while !shared.quit.load(Ordering::Relaxed) {
        let settings = shared.settings();
        let interval = Duration::from_secs(settings.interval);

        if Instant::now() >= directory_due {
            directory_due = Instant::now()
                + match queue::fetch_parks() {
                    Ok(found) if !found.is_empty() => {
                        store.inner.lock().unwrap().directory = found;
                        DIRECTORY_EVERY
                    }
                    _ => DIRECTORY_RETRY,
                };
        }

        // Work out what to fetch, holding the lock only briefly.
        let (jobs, sleep) = {
            let mut inner = store.inner.lock().unwrap();
            let now = Instant::now();
            for slug in &settings.parks {
                inner.entries.entry(slug.clone()).or_insert_with(Entry::new).wanted_until = Some(now + WANTED_FOR);
            }
            inner.entries.retain(|_, e| e.wanted_until.is_some_and(|t| t > now));

            let mut jobs = Vec::new();
            let mut sleep = interval;
            for (slug, e) in &inner.entries {
                let Some(park) = inner.directory.iter().find(|p| &p.slug == slug) else { continue };
                let queue_in = e.tried.map_or(Duration::ZERO, |t| interval.saturating_sub(now - t));
                let weather_in = e.weather_tried.map_or(Duration::ZERO, |t| WEATHER_EVERY.saturating_sub(now - t));
                if queue_in.is_zero() || weather_in.is_zero() {
                    jobs.push(Job { slug: slug.clone(), park: park.clone(), queue: queue_in.is_zero(), weather: weather_in.is_zero() });
                } else {
                    sleep = sleep.min(queue_in).min(weather_in);
                }
            }
            (jobs, sleep)
        };

        for job in &jobs {
            // Network calls happen without the lock held.
            let queue = job.queue.then(|| queue::fetch(job.park.id));
            let weather = job.weather.then(|| queue::fetch_weather(job.park.lat, job.park.lon));
            let mut inner = store.inner.lock().unwrap();
            let Some(e) = inner.entries.get_mut(&job.slug) else { continue };
            let now = Instant::now();
            if let Some(res) = queue {
                e.tried = Some(now);
                match res {
                    Ok(rides) => {
                        e.rides = rides;
                        e.fetched_at = Some(now_secs());
                        e.error = None;
                    }
                    Err(err) => e.error = Some(err),
                }
            }
            if let Some(res) = weather {
                e.weather_tried = Some(now);
                // A failed weather poll keeps the previous reading rather than blanking it.
                if let Ok(w) = res {
                    e.weather = Some(w);
                }
            }
            drop(inner);
            store.generation.fetch_add(1, Ordering::Relaxed);
        }

        if jobs.is_empty() {
            // Sleep until something is due, or until woken by a new request or settings change.
            let inner = store.inner.lock().unwrap();
            let _ = store.wake.wait_timeout(inner, sleep.max(Duration::from_millis(250)));
        }
    }
}
