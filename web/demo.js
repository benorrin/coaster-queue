// Made-up queue data for `?demo`, handy for developing the display or showing it off
// when the parks are shut. Loaded only when demo mode is on.

import { hashString } from './util.js';

/** Hand-written line-ups: [ride name, typical wait in minutes], where 0 is closed. */
const LINEUPS = {
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
const POOL = [
  'Big Dipper', 'Log Flume', 'The Dragon', 'Thunder Run', 'Cobra', 'Haunted Mansion', 'Pirate Ship',
  'River Rapids', 'Sky Drop', 'Mine Train', 'Spinning Teacups', 'Carousel', 'Ghost Train', 'Space Orbit',
  'Wild Mouse', 'Tower Twister', 'Safari Trail', 'Monorail', 'Bumper Cars', 'The Vortex', 'Rocket Coaster',
  'Water Splash', 'Dino Adventure', 'Fun House',
];

/** A small seeded random number generator (mulberry32), so a park's demo data is repeatable. */
function seeded(seed) {
  let t = seed;
  return () => {
    t = (t + 0x6d2b79f5) | 0;
    let r = Math.imul(t ^ (t >>> 15), 1 | t);
    r ^= r + Math.imul(r ^ (r >>> 7), 61 | r);
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
  };
}

function lineup(id) {
  if (LINEUPS[id]) return LINEUPS[id];
  const rand = seeded(hashString(id));
  const names = [...POOL].sort(() => rand() - 0.5).slice(0, 12 + Math.floor(rand() * 8));
  // The last couple of rides are closed.
  return names.map((name, i) => [name, i > names.length - 3 ? 0 : 5 * (1 + Math.floor(rand() * 11))]);
}

function weather(id) {
  const rand = seeded(hashString(`${id}/weather`));
  const code = [0, 1, 2, 3, 3, 61, 80, 45, 95][Math.floor(rand() * 9)];
  const temp = 6 + rand() * 17;
  return {
    temp_c: temp,
    feels_c: temp - 1.5 - rand() * 2,
    code,
    wind_mph: 4 + rand() * 16,
    rain_mm: code >= 61 && code < 90 ? rand() : 0,
  };
}

/**
 * Demo data for the given parks, shaped like the API's. Waits drift from `previous` (a map
 * of "park/ride" to the last wait) so repeated calls look like queues changing over time.
 */
export function demoParks(ids, nameOf, previous) {
  const now = Math.floor(Date.now() / 1000);
  return ids.map((id) => ({
    id,
    name: nameOf(id),
    fetched_at: now,
    error: null,
    weather: weather(id),
    rides: lineup(id)
      .map(([name, typical]) => {
        const was = previous.get(`${id}/${name}`) ?? typical;
        const drifted = Math.round((was + (Math.random() - 0.5) * 20) / 5) * 5;
        const wait = typical === 0 ? 0 : Math.max(5, Math.min(90, drifted));
        return { name, is_open: typical !== 0 || name === 'Submission', wait_time: wait };
      })
      .sort((a, b) => b.is_open - a.is_open || b.wait_time - a.wait_time || a.name.localeCompare(b.name)),
  }));
}
