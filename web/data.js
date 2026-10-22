const POLL_MS = 15000;

const DEMO_RIDES = {
  alton: [
    ['Nemesis', 35], ['Oblivion', 20], ['The Smiler', 45], ['Wicker Man', 55], ['Galactica', 30],
    ['Rita Queen of Speed', 25], ['TH13TEEN', 15], ['Spinball Whizzer', 20], ['Runaway Mine Train', 10],
    ['Gruffalo River Ride Adventure', 5], ['Congo River Rapids', 15], ['Hex - The Legend of the Towers', 25],
    ['Enterprise', 5], ['Duel - The Haunted House Strikes Back!', 10], ['Alton Mountain Rapids', 20],
    ['Submission', 0], ['Mutiny Bay', 0], ['Ripsaw', 5],
  ],
  thorpe: [
    ['Stealth', 50], ['Nemesis Inferno', 40], ['Colossus', 35], ['Saw - The Ride', 55], ['The Swarm', 30],
    ["Derren Brown's Ghost Train", 25], ['Tidal Wave', 20], ['Rush', 15], ['Depth Charge', 10],
    ['Flying Fish', 5], ['Zodiac', 5], ['Rumba Rapids', 10], ['Detonator', 20], ['Slammer', 25],
    ['Vortex', 10], ['Samurai', 5], ['Walking Dead: The Ride', 0], ['Quantum', 15],
  ],
};

/** Live ride data from the serve API; keeps the last good snapshot on failure. */
export class Feed {
  /** `demo` swaps the API for made-up queues that drift over time. */
  constructor({ demo = false } = {}) {
    this.demo = demo;
    this.parks = [];
    this.seen = new Map(); // ride key -> { wait, trend } from the previous poll
    this.loadedAt = 0; // ms, last successful poll
    this.error = null;
    this.poll();
    setInterval(() => this.poll(), POLL_MS);
  }

  pollDemo() {
    const now = Math.floor(Date.now() / 1000);
    const names = { alton: 'Alton Towers', thorpe: 'Thorpe Park' };
    const prev = new Map(this.parks.flatMap((p) => p.rides.map((r) => [`${p.id}/${r.name}`, r.wait_time])));
    this.parks = Object.entries(DEMO_RIDES).map(([id, rides]) => ({
      id,
      name: names[id],
      fetched_at: now,
      error: null,
      rides: rides
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
      const res = await fetch('/api/queues', { cache: 'no-store' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const parks = await res.json();
      this.markTrends(parks);
      this.parks = parks;
      this.loadedAt = Date.now();
      this.error = null;
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
