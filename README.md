# coaster-queue

Live ride wait times for UK theme parks (Alton Towers, Blackpool Pleasure Beach, Chessington, Flamingo
Land, Legoland Windsor, Paultons Park, Thorpe Park, and any UK park Queue-Times adds), from
[Queue-Times.com](https://queue-times.com/), with weather from [Open-Meteo](https://open-meteo.com/).

One small binary, three parts you can run in any mix:

| Part | What it is |
| --- | --- |
| `led` | The main display. Drawn in the terminal for now; this is what will drive a real LED matrix. |
| `api` | A JSON API for the data and the settings. |
| `web` | A browser display that looks like an RGB LED matrix. Includes the `api`. |

```
coaster-queue                      # led only (the default)
coaster-queue -r api               # headless API
coaster-queue -r led,api           # display plus API on one device
coaster-queue -r web               # API and web display (same as `serve`)
coaster-queue -r all
```

All parts share one data store and one poller, and the poller only fetches the parks somebody is looking
at, so running more parts costs almost nothing extra. Idle, the API uses under 5 MB of memory, two
threads and no CPU between polls; connections to the data sources are kept alive, and the server's
per-connection threads are small and capped.

## Choosing what to show

**Parks.** Name them on the command line (`coaster-queue chess legoland`; a unique start of the name is
enough), or use `parks` in the config. `--list-parks` shows the choices.

**Ride groups.** Restrict the display to some kinds of ride with `-S thrill,family` (or `show` in the
config). Built-in groups: `thrill`, `family`, `kids`, `water`, `transport`, `dark-rides`, `scare` and
`extras` (`--list-groups`). They match rides by name and by the land queue-times files them under, so new
parks pick up most of them automatically; the lists are curated by hand and will be imperfect, so
override anything that's wrong.

**Custom groups and ride lists** are defined in the config file, and are the same thing:

```toml
[groups.my-favourites]
rides = ["Nemesis*", "The Smiler", "Stealth"]       # a custom list of rides

[groups.big-day-out]
include = ["thrill", "transport"]                    # combine other groups
parks = ["alton-towers"]                             # ...for just this park
```

Patterns ignore case and may use `*`. A group can also match `lands`. Use one by adding its name to
`show`. A group with a built-in's name replaces it.

## Configuration

Settings are layered, each overriding the last: built-in defaults, the **config file**, **command-line
flags**, then the **API** while running (changes through the API last until the program restarts).

The config file is TOML, found at `--config FILE`, `./coaster-queue.toml`,
`~/.config/coaster-queue/config.toml` or `/etc/coaster-queue/config.toml`. See
[`coaster-queue.example.toml`](coaster-queue.example.toml) for every option. Unknown keys and invalid
values are errors, so typos are caught at startup. `--print-config` shows what's in effect.

| Setting | Meaning |
| --- | --- |
| `parks` | park slugs to show |
| `show` | ride groups to restrict to (empty: every ride) |
| `dwell` | seconds each page stays up |
| `show_closed` | include closed rides |
| `interval` | seconds between polls |
| `brightness`, `theme`, `gradients`, `weather`, `clock24`, `flip`, `shine`, `cols` | display options (used by the web display now, and by the LED matrix later) |

On the command line, `-s KEY=VALUE` sets any of them (`-s dwell=10 -s theme=amber`), and `-S`, `-a`
and `-i` are shorthands for `show`, `show_closed` and `interval`. Server options are `-b/--bind`,
`-p/--port`, `--web-dir` and `--api-token`. Run `coaster-queue --help` for everything.

## API

| Endpoint | |
| --- | --- |
| `GET /api/queues` | current data for the configured parks. `?parks=a,b` picks parks (`all` for every park), `?show=thrill,family` picks groups (`show=` for every ride) |
| `GET /api/parks` | every park available: `id` and `name` |
| `GET /api/groups` | every ride group: `id`, `label`, `builtin` |
| `GET /api/settings` | the current settings |
| `PATCH /api/settings` | change settings with a JSON object, e.g. `{"dwell": 12, "show": ["thrill"]}`. All or nothing; errors come back as `{"error": "..."}` with a 400 |

If `api_token` is set (config or `--api-token`), changes need `Authorization: Bearer <token>`; reads stay
open. Without a token anyone who can reach the port can change settings, so leave the server on
`127.0.0.1` or set a token before listening on other addresses.

## Web display

The `web` part serves a display that looks like an RGB LED matrix: text resolves like a split-flap board,
long names scroll, wait bars glide, arrows show which way a wait last moved, and the footer shows the
current weather for the park on screen. Press `s` (or the Settings link under the board) for a settings
menu drawn on the matrix, `p` to choose parks and `r` to choose ride groups.

The page starts from the server's settings. Changes made in a browser are remembered in that browser (and
in the URL, so a tuned board can be bookmarked), and only for settings that viewer changed; the rest keep
following the server. URL parameters: `parks`, `show`, `park`, `theme`, `dwell`, `brightness`, `cols`,
`gradients`, `weather`, `clock24`, `flip`, `shine`, `closed`, with `parks=all`/`show=all` spelling
"everything". Add `?demo` for made-up queues when the parks are shut.

| Key | Action |
| --- | --- |
| `space` | pause paging |
| `←` `→` | previous / next page (or tap the left third / the rest of the screen) |
| `↑` `↓` | brightness |
| `s` `p` `r` | settings menu, choose parks, choose ride groups |
| `t` `g` `c` `d` `w` | theme, gradients, closed rides, dot density, weather |
| `1`–`9` `0` | show just that chosen park / all chosen parks |
| `f` `h` | fullscreen, help |

## Terminal display (`led`)

Shows each chosen park in a panel, and takes turns when more are chosen than fit on screen. It redraws
only when data, settings or the clock change. Keys: `q` quit, `r` refresh now, `a` show or hide closed
rides.

## Credit

Wait times are provided by [Queue-Times.com](https://queue-times.com/) and weather by
[Open-Meteo.com](https://open-meteo.com/) (CC BY 4.0); keep both links on the web display.
