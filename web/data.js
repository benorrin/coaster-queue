// Live data from the server's API: ride waits for the chosen parks, plus the lists of
// parks and ride groups on offer.

import { getJson } from './util.js';

const POLL_MS = 15000;
const RETRY_MS = 3000; // before trying a failed list again
const LOADING_RETRY_MS = 2000; // before asking again while the server is still fetching a park

export class Feed {
  /**
   * `selected` returns the slugs of the parks to load and `shown` the ride groups to
   * restrict to (none means every ride). `demo` swaps the queue API for made-up queues.
   */
  constructor({ demo = false, selected = () => [], shown = () => [] } = {}) {
    this.demo = demo;
    this.selected = selected;
    this.shown = shown;

    this.parks = []; // data for the selected parks, in the API's shape
    this.directory = []; // every park the server offers: { id, name }
    this.groups = []; // every ride group the server offers: { id, label }
    this.names = new Map(); // park id -> display name
    this.error = null; // why the last poll failed, if it did
    this.onDirectory = null; // called once the park list arrives
    this.onGroups = null; // called once the group list arrives
    this.seen = new Map(); // ride key -> { wait, trend } from the previous poll

    this.#loadList('/api/parks', (list) => {
      this.directory = list;
      this.names = new Map(list.map((p) => [p.id, p.name]));
      this.onDirectory?.(list);
    });
    this.#loadList('/api/groups', (list) => {
      this.groups = list;
      this.onGroups?.(list);
    });
    this.poll();
    setInterval(() => this.poll(), POLL_MS);
  }

  /** Fetch a list from the server, retrying until it arrives. */
  async #loadList(path, assign) {
    try {
      assign(await getJson(path));
    } catch {
      setTimeout(() => this.#loadList(path, assign), RETRY_MS);
    }
  }

  /** A park's display name, falling back to something readable from its slug. */
  nameOf(id) {
    return this.names.get(id) ?? id.replace(/-/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
  }

  /** Fetch the latest data now; the previous data stays on screen if this fails. */
  async poll() {
    try {
      const parks = this.demo ? await this.#demoParks() : await this.#fetchParks();
      this.#markTrends(parks);
      this.parks = parks;
      this.error = null;
      // Parks the server hasn't fetched yet: look again soon rather than in 15s.
      clearTimeout(this.retry);
      if (!this.demo && (parks.length < this.selected().length || parks.some((p) => !p.fetched_at))) {
        this.retry = setTimeout(() => this.poll(), LOADING_RETRY_MS);
      }
    } catch (e) {
      this.error = String(e.message || e);
    }
  }

  #fetchParks() {
    return getJson(`/api/queues?parks=${this.selected().join(',')}&show=${this.shown().join(',')}`);
  }

  async #demoParks() {
    const { demoParks } = await import('./demo.js');
    const previous = new Map(this.parks.flatMap((p) => p.rides.map((r) => [`${p.id}/${r.name}`, r.wait_time])));
    return demoParks(this.selected(), (id) => this.nameOf(id), previous);
  }

  /**
   * Tag each ride with `trend` (+1 up, -1 down) from the last poll where its wait
   * changed; it sticks until the wait moves again.
   */
  #markTrends(parks) {
    const next = new Map();
    for (const park of parks) {
      for (const ride of park.rides) {
        const key = `${park.id}/${ride.name}`;
        const before = this.seen.get(key);
        if (before === undefined) ride.trend = 0;
        else ride.trend = ride.wait_time === before.wait ? before.trend : Math.sign(ride.wait_time - before.wait);
        next.set(key, { wait: ride.wait_time, trend: ride.trend });
      }
    }
    this.seen = next;
  }
}
