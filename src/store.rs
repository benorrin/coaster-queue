//! Data shared by every component: the settings, the ride groups and the latest queue
//! times and weather. One poller thread keeps the store fresh, fetching only the parks
//! somebody currently wants (the configured parks, plus any a client has asked the API
//! for recently) so a small board makes as few requests as it can.

use crate::groups::{Filter, Groups};
use crate::queue::{self, Park, Ride, Weather};
use crate::settings::{Patch, Settings};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex, MutexGuard, RwLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// How long a park keeps being polled after a client last asked for it.
const WANTED_FOR: Duration = Duration::from_secs(600);
const WEATHER_EVERY: Duration = Duration::from_secs(600);
const DIRECTORY_EVERY: Duration = Duration::from_secs(24 * 3600);
const DIRECTORY_RETRY: Duration = Duration::from_secs(60);
/// The poller never sleeps for less than this, so a burst of wake-ups can't spin it.
const MIN_SLEEP: Duration = Duration::from_millis(250);

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

// ---- shared state -----------------------------------------------------------

/// Everything the components share. Wrapped in an `Arc` and handed to each of them.
pub struct Shared {
    settings: RwLock<Settings>,
    groups: RwLock<Groups>,
    pub store: Store,
    /// Bumped whenever the settings change, so displays know to redraw.
    settings_gen: AtomicU64,
    /// Set when the main display exits, to stop the poller.
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

    /// A snapshot of the current settings.
    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn settings_gen(&self) -> u64 {
        self.settings_gen.load(Ordering::Relaxed)
    }

    /// Run `f` with the ride groups.
    pub fn groups<R>(&self, f: impl FnOnce(&Groups) -> R) -> R {
        f(&self.groups.read().unwrap())
    }

    /// A ride filter for the named groups (all rides if none).
    pub fn filter(&self, show: &[String]) -> Result<Filter, String> {
        self.groups(|g| g.compile(show))
    }

    /// Apply a change to the live settings, checking that the parks and groups it names exist.
    pub fn update(&self, patch: Patch) -> Result<Settings, String> {
        let mut next = self.settings();
        next.apply(patch)?;
        if let Some(bad) = next.parks.iter().find(|p| !self.store.has_park(p)) {
            return Err(format!("unknown park {bad:?}"));
        }
        self.filter(&next.show)?;
        *self.settings.write().unwrap() = next.clone();
        self.settings_gen.fetch_add(1, Ordering::Relaxed);
        self.store.wake();
        Ok(next)
    }
}

// ---- the store --------------------------------------------------------------

/// What's known about one park. `tried` records when a fetch was last *attempted*, so a
/// failing park is retried once per interval rather than in a tight loop.
#[derive(Default)]
struct Entry {
    rides: Vec<Ride>,
    fetched_at: Option<u64>,
    tried: Option<Instant>,
    error: Option<String>,
    weather: Option<Weather>,
    weather_tried: Option<Instant>,
    /// Polled until this time unless it's one of the configured parks.
    wanted_until: Option<Instant>,
}

struct Inner {
    directory: Vec<Park>,
    entries: HashMap<String, Entry>,
}

pub struct Store {
    inner: Mutex<Inner>,
    /// Wakes the poller early: a new park was asked for, settings changed, or a refresh.
    wake: Condvar,
    /// Bumped whenever fresh data arrives.
    generation: AtomicU64,
}

/// One park's data as seen through a ride filter; this is what the API serves.
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

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Every park on offer as (slug, name).
    pub fn directory(&self) -> Vec<(String, String)> {
        self.lock().directory.iter().map(|p| (p.slug.clone(), p.name.clone())).collect()
    }

    pub fn has_park(&self, slug: &str) -> bool {
        self.lock().directory.iter().any(|p| p.slug == slug)
    }

    /// The park's current data with `filter` applied, or None if the park is unknown.
    pub fn view(&self, slug: &str, filter: &Filter) -> Option<ParkView> {
        let inner = self.lock();
        let park = inner.directory.iter().find(|p| p.slug == slug)?;
        let blank = Entry::default();
        let entry = inner.entries.get(slug).unwrap_or(&blank);
        Some(ParkView {
            slug: park.slug.clone(),
            name: park.name.clone(),
            rides: entry.rides.iter().filter(|r| filter.allows(slug, r)).cloned().collect(),
            fetched_at: entry.fetched_at,
            error: entry.error.clone(),
            weather: entry.weather.clone(),
        })
    }

    /// Note that a client wants these parks, so they get polled for the next few minutes.
    pub fn want(&self, slugs: &[String]) {
        let mut inner = self.lock();
        let until = Instant::now() + WANTED_FOR;
        let mut unseen = false;
        for slug in slugs {
            if inner.directory.iter().any(|p| &p.slug == slug) {
                let entry = inner.entries.entry(slug.clone()).or_default();
                unseen |= entry.tried.is_none();
                entry.wanted_until = Some(until);
            }
        }
        drop(inner);
        if unseen {
            self.wake();
        }
    }

    /// Poll everything again now.
    pub fn refresh(&self) {
        self.lock().entries.values_mut().for_each(|e| e.tried = None);
        self.wake();
    }

    pub fn wake(&self) {
        self.wake.notify_all();
    }

    /// Decide what the poller should fetch now, and how long it can sleep if nothing is due.
    /// Also marks the configured parks as wanted and forgets parks nobody wants any more.
    fn plan(&self, configured: &[String], interval: Duration) -> (Vec<Job>, Duration) {
        let mut inner = self.lock();
        let now = Instant::now();
        for slug in configured {
            inner.entries.entry(slug.clone()).or_default().wanted_until = Some(now + WANTED_FOR);
        }
        inner.entries.retain(|_, e| e.wanted_until.is_some_and(|until| until > now));

        let mut jobs = Vec::new();
        let mut sleep = interval;
        for (slug, entry) in &inner.entries {
            let Some(park) = inner.directory.iter().find(|p| &p.slug == slug) else { continue };
            let queue_in = remaining(entry.tried, interval, now);
            let weather_in = remaining(entry.weather_tried, WEATHER_EVERY, now);
            if queue_in.is_zero() || weather_in.is_zero() {
                jobs.push(Job {
                    park: park.clone(),
                    queue: queue_in.is_zero(),
                    weather: weather_in.is_zero(),
                });
            } else {
                sleep = sleep.min(queue_in).min(weather_in);
            }
        }
        (jobs, sleep)
    }

    /// Store the results of a job's fetches.
    fn record(
        &self,
        slug: &str,
        queue: Option<Result<Vec<Ride>, String>>,
        weather: Option<Result<Weather, String>>,
    ) {
        let mut inner = self.lock();
        let Some(entry) = inner.entries.get_mut(slug) else { return };
        let now = Instant::now();
        if let Some(result) = queue {
            entry.tried = Some(now);
            match result {
                Ok(rides) => {
                    entry.rides = rides;
                    entry.fetched_at = Some(now_secs());
                    entry.error = None;
                }
                // Keep the last good rides on screen; the error is shown alongside.
                Err(e) => entry.error = Some(e),
            }
        }
        if let Some(result) = weather {
            entry.weather_tried = Some(now);
            // A failed weather poll keeps the previous reading rather than blanking it.
            if let Ok(w) = result {
                entry.weather = Some(w);
            }
        }
        drop(inner);
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}

/// Time left until something last done at `last` is due again after `every` (zero if due now).
fn remaining(last: Option<Instant>, every: Duration, now: Instant) -> Duration {
    last.map_or(Duration::ZERO, |t| every.saturating_sub(now - t))
}

// ---- the poller -------------------------------------------------------------

/// A park's fetches that are due.
struct Job {
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

        if Instant::now() >= directory_due {
            directory_due = Instant::now() + refresh_directory(store);
        }

        let (jobs, sleep) = store.plan(&settings.parks, Duration::from_secs(settings.interval));
        for job in &jobs {
            // The network calls happen without any lock held.
            let queue = job.queue.then(|| queue::fetch(job.park.id));
            let weather = job.weather.then(|| queue::fetch_weather(job.park.lat, job.park.lon));
            store.record(&job.park.slug, queue, weather);
        }

        if jobs.is_empty() {
            // Sleep until something is due, or until woken by a new request or settings change.
            let _ = store.wake.wait_timeout(store.lock(), sleep.max(MIN_SLEEP));
        }
    }
}

/// Reload the park directory; returns how long until it should be reloaded.
fn refresh_directory(store: &Store) -> Duration {
    match queue::fetch_parks() {
        Ok(found) if !found.is_empty() => {
            store.lock().directory = found;
            DIRECTORY_EVERY
        }
        _ => DIRECTORY_RETRY,
    }
}
