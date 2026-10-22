# merlin-queue

Live ride wait times for Alton Towers and Thorpe Park, from [Queue-Times.com](https://queue-times.com/).
Two front ends share one binary: a terminal UI and a web page that shows the queues on a simulated RGB LED matrix.

## Terminal

```
merlin-queue [alton] [thorpe] [-a|--all] [-i|--interval SECS]
```

Keys: `q` quit, `r` refresh, `a` toggle closed rides.

## Web board

```
merlin-queue serve [-b ADDR] [-p PORT] [-i SECS] [--web-dir DIR]
```

Serves the board at <http://127.0.0.1:8080>. The server polls queue-times in the background
(default every 30s) and the page reads the cached result from `/api/queues`, so any number of
viewers costs one upstream request per park per interval. The front end is embedded in the binary;
`--web-dir web` reads it from disk instead while you work on it.

The board pages through each park's open rides, longest wait first. Text resolves like a split-flap
display, long names scroll, wait bars glide, arrows show which way a wait last moved, and a ticker
along the bottom summarises each park.

| Key | Action |
| --- | --- |
| `space` | pause paging |
| `←` `→` | previous / next page (or tap the left third / the rest of the screen) |
| `t` | theme: rgb, amber, green, ice, rainbow |
| `↑` `↓` | brightness |
| `g` `d` | glow, dot density |
| `c` | show closed rides |
| `1` `2` `0` | one park / all parks |
| `f` | fullscreen |
| `h` | help |

Settings are kept in the URL, so a tuned board can be bookmarked for a wall-mounted screen:
`?theme=amber&park=thorpe&dwell=6&brightness=0.8&cols=256&closed=1`. Add `?demo` for made-up queues,
handy when the parks are shut.

## Credit

Wait times are provided by [Queue-Times.com](https://queue-times.com/); keep the link on the page.
