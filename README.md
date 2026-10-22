# coaster-queue

Live ride wait times for Alton Towers and Thorpe Park, from [Queue-Times.com](https://queue-times.com/).
Two front ends share one binary: a terminal UI and a web page that shows the queues on a simulated RGB LED matrix.

## Terminal

```
coaster-queue [alton] [thorpe] [-a|--all] [-i|--interval SECS]
```

Keys: `q` quit, `r` refresh, `a` toggle closed rides.

## Web board

```
coaster-queue serve [-b ADDR] [-p PORT] [-i SECS] [--web-dir DIR]
```

Serves the board at <http://127.0.0.1:8080>. The server polls queue-times in the background
(default every 30s) and the page reads the cached result from `/api/queues`, so any number of
viewers costs one upstream request per park per interval. The front end is embedded in the binary;
`--web-dir web` reads it from disk instead while you work on it.

The board pages through each park's open rides, longest wait first. Text resolves like a split-flap
display, long names scroll, wait bars glide, and arrows show which way a wait last moved. The footer
shows the current weather for whichever park is on screen (from [Open-Meteo](https://open-meteo.com/),
refreshed every 10 minutes).

Press `s` (or the Settings link under the board) for the settings menu: page time, theme, brightness,
dot density, gradients, closed rides, park, weather, 12/24-hour clock, flip text and shine sweep.
Use the arrow keys, or tap a row's left/right half to step its value. Settings are remembered in the
browser.

| Key | Action |
| --- | --- |
| `space` | pause paging |
| `←` `→` | previous / next page (or tap the left third / the rest of the screen) |
| `↑` `↓` | brightness |
| `s` | settings menu |
| `t` `g` `c` `d` `w` | theme, gradients, closed rides, dot density, weather |
| `1` `2` `0` | one park / all parks |
| `f` | fullscreen |
| `h` | help |

Settings are also kept in the URL, so a tuned board can be bookmarked for a wall-mounted screen, and
URL values win over remembered ones:
`?theme=amber&park=thorpe&dwell=6&brightness=0.8&cols=256&gradients=0&weather=0&clock24=0&closed=1`.
Add `?demo` for made-up queues, handy when the parks are shut.

## Credit

Wait times are provided by [Queue-Times.com](https://queue-times.com/) and weather by
[Open-Meteo.com](https://open-meteo.com/) (CC BY 4.0); keep both links on the page.
