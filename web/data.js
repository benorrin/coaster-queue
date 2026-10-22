const POLL_MS = 15000;

const DEMO_RIDES = {
  'alton-towers': [
    ['Nemesis', 35], ['Oblivion', 20], ['The Smiler', 45], ['Wicker Man', 55], ['Galactica', 30],
    ['Rita Queen of Speed', 25], ['TH13TEEN', 15], ['Spinball Whizzer', 20], ['Runaway Mine Train', 10],
    ['Gruffalo River Ride Adventure', 5], ['Congo River Rapids', 15], ['Hex - The Legend of the Towers', 25],
    ['Enterprise', 5], ['Duel - The Haunted House Strikes Back!', 10], ['Alton Mountain Rapids', 20],
    ['Submission', 0], ['Mutiny Bay', 0], ['Ripsaw', 5],
  ],
  'thorpe-park': [
    ['Stealth', 50], ['Nemesis Inferno', 40], ['Colossus', 35], ['Saw - The Ride', 55], ['The Swarm', 30],
    ["Derren Brown's Ghost Train", 25], ['Tidal Wave', 20], ['Rush', 15], ['Depth Charge', 10],
    ['Flying Fish', 5], ['Zodiac', 5], ['Rumba Rapids', 10], ['Detonator', 20], ['Slammer', 25],
    ['Vortex', 10], ['Samurai', 5], ['Walking Dead: The Ride', 0], ['Quantum', 15],
  ],
};

// Other parks get a repeatable made-up line-up built from these.
const DEMO_POOL = [
  'Big Dipper', 'Log Flume', 'The Dragon', 'Thunder Run', 'Cobra', 'Haunted Mansion', 'Pirate Ship',
  'River Rapids', 'Sky Drop', 'Mine Train', 'Spinning Teacups', 'Carousel', 'Ghost Train', 'Space Orbit',
  'Wild Mouse', 'Tower Twister', 'Safari Trail', 'Monorail', 'Bumper Cars', 'The Vortex', 'Rocket Coaster',
  'Water Splash', 'Dino Adventure', 'Fun House',
];

function hashString(s) {
  let h = 2166136261;
  for (const c of s) h = Math.imul(h ^ c.charCodeAt(0), 16777619);
  return h >>> 0;
}

function seeded(seed) {
  let t = seed;
  return () => {
    t = (t + 0x6d2b79f5) | 0;
    let r = Math.imul(t ^ (t >>> 15), 1 | t);
    r ^= r + Math.imul(r ^ (r >>> 7), 61 | r);
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
  };
}

function demoLineup(id) {
  if (DEMO_RIDES[id]) return DEMO_RIDES[id];
  const rand = seeded(hashString(id));
  const names = [...DEMO_POOL].sort(() => rand() - 0.5).slice(0, 12 + Math.floor(rand() * 8));
  return names.map((name, i) => [name, i > names.length - 3 ? 0 : 5 * (1 + Math.floor(rand() * 11))]);
}

function demoWeather(id) {
  const rand = seeded(hashString(`${id}/weather`));
  const code = [0, 1, 2, 3, 3, 61, 80, 45, 95][Math.floor(rand() * 9)];
  const temp = 6 + rand() * 17;
  return { temp_c: temp, feels_c: temp - 1.5 - rand() * 2, code, wind_mph: 4 + rand() * 16, rain_mm: code >= 61 && code < 90 ? rand() : 0 };
}

/** Live ride data from the serve API; keeps the last good snapshot on failure. */
export class Feed {
  /**
   * `selected` returns the slugs of the parks to load. `demo` swaps the queue
   * API for made-up queues that drift over time.
   */
  constructor({ demo = false, selected = () => [], shown = () => [] } = {}) {
    this.demo = demo;
    this.selected = selected;
    this.shown = shown; // ride groups to restrict to; none means every ride
    this.groups = []; // ride groups the server offers: { id, label }
    this.onGroups = null;
    this.directory = []; // every park the server offers: { id, name }
    this.names = new Map(); // id -> display name
    this.onDirectory = null; // called once the park list arrives
    this.parks = [];
    this.seen = new Map(); // ride key -> { wait, trend } from the previous poll
    this.loadedAt = 0; // ms, last successful poll
    this.error = null;
    this.loadDirectory();
    this.loadGroups();
    this.poll();
    setInterval(() => this.poll(), POLL_MS);
  }

  async loadDirectory() {
    try {
      const res = await fetch('/api/parks', { cache: 'no-store' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      this.directory = await res.json();
      this.names = new Map(this.directory.map((p) => [p.id, p.name]));
      this.onDirectory?.(this.directory);
    } catch {
      setTimeout(() => this.loadDirectory(), 3000);
    }
  }

  async loadGroups() {
    try {
      const res = await fetch('/api/groups', { cache: 'no-store' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      this.groups = await res.json();
      this.onGroups?.(this.groups);
    } catch {
      setTimeout(() => this.loadGroups(), 3000);
    }
  }

  /** A park's display name, falling back to something readable from its slug. */
  nameOf(id) {
    return this.names.get(id) ?? id.replace(/-/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
  }

  pollDemo() {
    const now = Math.floor(Date.now() / 1000);
    const prev = new Map(this.parks.flatMap((p) => p.rides.map((r) => [`${p.id}/${r.name}`, r.wait_time])));
    this.parks = this.selected().map((id) => ({
      id,
      name: this.nameOf(id),
      fetched_at: now,
      error: null,
      weather: demoWeather(id),
      rides: demoLineup(id)
        .map(([name, base]) => {
          const was = prev.get(`${id}/${name}`) ?? base;
          const wait = base === 0 ? 0 : Math.max(5, Math.min(90, Math.round((was + (Math.random() - 0.5) * 20) / 5) * 5));
          return { name, is_open: base !== 0 || name === 'Submission', wait_time: wait };
        })
        .sort((a, b) => b.is_open - a.is_open || b.wait_time - a.wait_time || a.name.localeCompare(b.name)),
    }));
    this.markTrends(this.parks);
    this.loadedAt = Date.now();
  }

  async poll() {
    if (this.demo) return this.pollDemo();
    try {
      const res = await fetch(`/api/queues?parks=${this.selected().join(',')}&show=${this.shown().join(',')}`, { cache: 'no-store' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const parks = await res.json();
      this.markTrends(parks);
      this.parks = parks;
      this.loadedAt = Date.now();
      this.error = null;
      // Parks the server hasn't fetched yet: look again soon rather than in 15s.
      clearTimeout(this.retry);
      if (parks.length < this.selected().length || parks.some((p) => !p.fetched_at)) {
        this.retry = setTimeout(() => this.poll(), 2000);
      }
    } catch (e) {
      this.error = String(e.message || e);
    }
  }

  /**
   * Tag each ride with `trend` (+1 up, -1 down) from the last poll where its
   * wait changed; it sticks until the wait moves again.
   */
  markTrends(parks) {
    const next = new Map();
    for (const park of parks) {
      for (const ride of park.rides) {
        const key = `${park.id}/${ride.name}`;
        const before = this.seen.get(key);
        ride.trend = before === undefined ? 0 : ride.wait_time === before.wait ? before.trend : Math.sign(ride.wait_time - before.wait);
        next.set(key, { wait: ride.wait_time, trend: ride.trend });
      }
    }
    this.seen = next;
  }

  /** Seconds since the newest upstream fetch, or null if nothing has loaded. */
  age() {
    const newest = Math.max(0, ...this.parks.map((p) => p.fetched_at ?? 0));
    return newest ? Math.max(0, Date.now() / 1000 - newest) : null;
  }
}
