//! Command line and config file. Settings are layered: built-in defaults, then the
//! config file, then command-line flags (and then the API, while running).

use crate::groups::GroupDef;
use crate::queue::{self, Park};
use crate::settings::{Patch, Settings};
use serde::Deserialize;
use std::{collections::BTreeMap, env, fs, path::PathBuf};

pub const HELP: &str = "\
coaster-queue - live ride wait times for UK theme parks

usage: coaster-queue [OPTIONS] [PARK...]

what to run:
  -r, --run LIST        led, api, web, or all, comma separated (default: led).
                        led is the main display (the terminal for now, an LED matrix
                        later); web is the browser display and includes the api.
      serve             shorthand for --run api,web

settings (also available in the config file and through the API):
  PARK...               parks to show: names from --list-parks or the start of one
  -S, --show GROUPS     only rides in these groups, e.g. thrill,family (--list-groups)
  -a, --all             include closed rides
  -i, --interval SECS   seconds between queue-time polls
  -s, --set KEY=VALUE   set any setting, repeatable (see --print-config), e.g. -s dwell=10

server (api and web):
  -b, --bind ADDR       address to listen on (default 127.0.0.1)
  -p, --port PORT       port to listen on (default 8080)
      --web-dir DIR     serve the web display from DIR instead of the built-in copy
      --api-token TOKEN require this bearer token for API changes

other:
  -c, --config FILE     config file (default: ./coaster-queue.toml, then
                        ~/.config/coaster-queue/config.toml, then /etc/coaster-queue/config.toml)
  -l, --list-parks      list the parks available
  -g, --list-groups     list the ride groups available
      --print-config    show the effective configuration and exit
  -h, --help            show this help

led keys: q quit - r refresh now - a show/hide closed rides
";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    pub led: bool,
    pub api: bool,
    pub web: bool,
}

impl Run {
    fn parse(list: &[String]) -> Result<Run, String> {
        let mut run = Run { led: false, api: false, web: false };
        for part in list {
            match part.to_ascii_lowercase().as_str() {
                "led" => run.led = true,
                "api" => run.api = true,
                "web" => run.web = true,
                "all" => run = Run { led: true, api: true, web: true },
                other => return Err(format!("unknown component {other:?} (use led, api, web or all)")),
            }
        }
        if run.web {
            run.api = true; // the web display reads from the api
        }
        if !(run.led || run.api || run.web) {
            return Err("nothing to run".into());
        }
        Ok(run)
    }

    pub fn names(&self) -> Vec<&'static str> {
        [(self.led, "led"), (self.api, "api"), (self.web, "web")]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| n)
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct Server {
    pub bind: String,
    pub port: u16,
    pub web_dir: Option<PathBuf>,
    pub api_token: Option<String>,
    pub max_connections: usize,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerPatch {
    bind: Option<String>,
    port: Option<u16>,
    web_dir: Option<PathBuf>,
    api_token: Option<String>,
    max_connections: Option<usize>,
}

impl ServerPatch {
    fn apply(self, s: &mut Server) {
        s.bind = self.bind.unwrap_or(std::mem::take(&mut s.bind));
        s.port = self.port.unwrap_or(s.port);
        s.web_dir = self.web_dir.or(s.web_dir.take());
        s.api_token = self.api_token.or(s.api_token.take());
        s.max_connections = self.max_connections.unwrap_or(s.max_connections);
    }
}

pub struct Config {
    pub run: Run,
    pub server: Server,
    pub settings: Settings,
    pub custom_groups: BTreeMap<String, GroupDef>,
    pub path: Option<PathBuf>,
}

pub enum Command {
    Run(Config),
    ListParks,
    ListGroups(Config),
    PrintConfig(Config),
    Help,
}

struct Args {
    run: Option<Vec<String>>,
    config: Option<PathBuf>,
    sets: Vec<(String, String)>,
    names: Vec<String>,
    server: ServerPatch,
    list_parks: bool,
    list_groups: bool,
    print_config: bool,
    help: bool,
}

fn parse_args(raw: Vec<String>) -> Result<Args, String> {
    let mut a = Args {
        run: None,
        config: None,
        sets: Vec::new(),
        names: Vec::new(),
        server: ServerPatch::default(),
        list_parks: false,
        list_groups: false,
        print_config: false,
        help: false,
    };
    let mut it = raw.into_iter();
    let mut first = true;
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "serve" if first => a.run = Some(vec!["api".into(), "web".into()]),
            "-r" | "--run" => a.run = Some(value("--run")?.split(',').map(|s| s.trim().to_string()).collect()),
            "-c" | "--config" => a.config = Some(value("--config")?.into()),
            "-s" | "--set" => {
                let kv = value("--set")?;
                let (k, v) = kv.split_once('=').ok_or_else(|| format!("--set needs KEY=VALUE, not {kv:?}"))?;
                a.sets.push((k.trim().to_string(), v.to_string()));
            }
            "-S" | "--show" => a.sets.push(("show".into(), value("--show")?)),
            "-a" | "--all" => a.sets.push(("show_closed".into(), "true".into())),
            "-i" | "--interval" => a.sets.push(("interval".into(), value("--interval")?)),
            "-b" | "--bind" => a.server.bind = Some(value("--bind")?),
            "-p" | "--port" => {
                a.server.port = Some(value("--port")?.parse().map_err(|_| "--port needs a number".to_string())?)
            }
            "--web-dir" => a.server.web_dir = Some(value("--web-dir")?.into()),
            "--api-token" => a.server.api_token = Some(value("--api-token")?),
            "-l" | "--list-parks" => a.list_parks = true,
            "-g" | "--list-groups" => a.list_groups = true,
            "--print-config" => a.print_config = true,
            "-h" | "--help" => a.help = true,
            other if other.starts_with('-') => return Err(format!("unknown option: {other} (see --help)")),
            other => a.names.push(other.to_string()),
        }
        first = false;
    }
    Ok(a)
}

fn config_candidates() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from("coaster-queue.toml")];
    if let Some(dir) = env::var_os("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(dir).join("coaster-queue/config.toml"));
    } else if let Some(home) = env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".config/coaster-queue/config.toml"));
    }
    paths.push(PathBuf::from("/etc/coaster-queue/config.toml"));
    paths
}

/// Apply the contents of a config file to `cfg`. Unknown keys are errors, so typos are caught.
fn apply_file(cfg: &mut Config, text: &str, run: &mut Option<Vec<String>>) -> Result<(), String> {
    let mut table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    if let Some(v) = table.remove("run") {
        *run = Some(v.try_into().map_err(|e: toml::de::Error| format!("run: {e}"))?);
    }
    if let Some(v) = table.remove("server") {
        let patch: ServerPatch = v.try_into().map_err(|e: toml::de::Error| format!("[server]: {e}"))?;
        patch.apply(&mut cfg.server);
    }
    if let Some(v) = table.remove("groups") {
        cfg.custom_groups = v.try_into().map_err(|e: toml::de::Error| format!("[groups]: {e}"))?;
    }
    let patch: Patch = toml::Value::Table(table).try_into().map_err(|e: toml::de::Error| e.to_string())?;
    cfg.settings.apply(patch)
}

/// Resolve a name given on the command line against park slugs: exactly, or as the
/// start of exactly one.
pub fn find_park(directory: &[Park], name: &str) -> Result<Park, String> {
    let name = name.to_lowercase();
    if let Some(p) = directory.iter().find(|p| p.slug == name) {
        return Ok(p.clone());
    }
    let matches: Vec<&Park> = directory.iter().filter(|p| p.slug.starts_with(&name)).collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!("unknown park: {name} (see --list-parks)")),
        many => Err(format!(
            "{name} matches several parks: {}",
            many.iter().map(|p| p.slug.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// The UK parks queue-times knows, falling back to the built-in two if it can't be reached.
pub fn directory() -> Vec<Park> {
    queue::fetch_parks().unwrap_or_else(|e| {
        eprintln!("warning: couldn't load the park list ({e}); only Alton Towers and Thorpe Park are available");
        queue::default_parks()
    })
}

pub fn load(raw: Vec<String>) -> Result<Command, String> {
    let args = parse_args(raw)?;
    if args.help {
        return Ok(Command::Help);
    }
    if args.list_parks {
        return Ok(Command::ListParks);
    }

    let mut cfg = Config {
        run: Run { led: true, api: false, web: false },
        server: Server { bind: "127.0.0.1".into(), port: 8080, web_dir: None, api_token: None, max_connections: 32 },
        settings: Settings::default(),
        custom_groups: BTreeMap::new(),
        path: None,
    };

    let mut run = None;
    match &args.config {
        Some(path) => {
            let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            apply_file(&mut cfg, &text, &mut run).map_err(|e| format!("{}: {e}", path.display()))?;
            cfg.path = Some(path.clone());
        }
        None => {
            for path in config_candidates() {
                if let Ok(text) = fs::read_to_string(&path) {
                    apply_file(&mut cfg, &text, &mut run).map_err(|e| format!("{}: {e}", path.display()))?;
                    cfg.path = Some(path);
                    break;
                }
            }
        }
    }

    // Command-line flags win over the file.
    if let Some(r) = args.run {
        run = Some(r);
    }
    if let Some(r) = run {
        cfg.run = Run::parse(&r)?;
    }
    args.server.apply(&mut cfg.server);
    cfg.settings.apply(Patch::from_pairs(&args.sets)?)?;
    if !args.names.is_empty() {
        let directory = directory();
        let parks: Result<Vec<String>, String> =
            args.names.iter().map(|n| find_park(&directory, n).map(|p| p.slug)).collect();
        cfg.settings.apply(Patch { parks: Some(parks?), ..Patch::default() })?;
    }

    Ok(if args.list_groups {
        Command::ListGroups(cfg)
    } else if args.print_config {
        Command::PrintConfig(cfg)
    } else {
        Command::Run(cfg)
    })
}

/// The effective configuration as TOML, for --print-config.
pub fn print(cfg: &Config) -> String {
    let mut out = String::new();
    match &cfg.path {
        Some(p) => out.push_str(&format!("# config file: {}\n", p.display())),
        None => out.push_str("# no config file found; defaults and command-line flags only\n"),
    }
    let run: Vec<String> = cfg.run.names().iter().map(|n| format!("{n:?}")).collect();
    out.push_str(&format!("run = [{}]\n", run.join(", ")));
    out.push_str(&toml::to_string(&cfg.settings).unwrap_or_default());
    out.push_str("\n[server]\n");
    out.push_str(&format!("bind = {:?}\nport = {}\nmax_connections = {}\n", cfg.server.bind, cfg.server.port, cfg.server.max_connections));
    if let Some(d) = &cfg.server.web_dir {
        out.push_str(&format!("web_dir = {:?}\n", d.display().to_string()));
    }
    out.push_str(&format!("api_token = {}\n", if cfg.server.api_token.is_some() { "\"(set)\"" } else { "\"\"  # none: API changes are open" }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn run_components() {
        let r = Run::parse(&args(&["led", "web"])).unwrap();
        assert!(r.led && r.web && r.api, "web implies api");
        assert!(Run::parse(&args(&["all"])).unwrap().led);
        assert!(Run::parse(&args(&["tv"])).is_err());
    }

    #[test]
    fn file_then_flags() {
        let mut cfg = Config {
            run: Run { led: true, api: false, web: false },
            server: Server { bind: "127.0.0.1".into(), port: 8080, web_dir: None, api_token: None, max_connections: 32 },
            settings: Settings::default(),
            custom_groups: BTreeMap::new(),
            path: None,
        };
        let mut run = None;
        apply_file(
            &mut cfg,
            r#"
            run = ["api"]
            parks = ["thorpe-park"]
            dwell = 5
            [server]
            port = 9000
            [groups.favs]
            rides = ["Stealth"]
            "#,
            &mut run,
        )
        .unwrap();
        assert_eq!(run.unwrap(), ["api"]);
        assert_eq!((cfg.settings.dwell, cfg.server.port), (5, 9000));
        assert_eq!(cfg.settings.parks, ["thorpe-park"]);
        assert!(cfg.custom_groups.contains_key("favs"));
        assert!(apply_file(&mut cfg, "dwel = 5", &mut None).is_err(), "typos are errors");
        assert!(apply_file(&mut cfg, "dwell = 1", &mut None).is_err(), "values are validated");
    }

    #[test]
    fn parses_flags() {
        let a = parse_args(args(&["-r", "led,api", "-a", "-s", "dwell=9", "chess", "-S", "thrill"])).unwrap();
        assert_eq!(a.run.unwrap(), ["led", "api"]);
        assert_eq!(a.names, ["chess"]);
        assert_eq!(a.sets.len(), 3);
        assert!(parse_args(args(&["--nope"])).is_err());
        assert!(parse_args(args(&["-s", "noequals"])).is_err());
        assert_eq!(parse_args(args(&["serve", "-p", "9"])).unwrap().run.unwrap(), ["api", "web"]);
    }
}
