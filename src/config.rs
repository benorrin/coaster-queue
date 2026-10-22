//! Command line and config file. Settings are layered: built-in defaults, then the
//! config file, then command-line flags (and then the API, while running).

use crate::groups::GroupDef;
use crate::queue;
use crate::settings::{Patch, Settings};
use serde::{de::DeserializeOwned, Deserialize};
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

/// Which parts of the program to run.
#[derive(Clone, Copy)]
pub struct Run {
    pub led: bool,
    pub api: bool,
    pub web: bool,
}

impl Run {
    /// Parse a component list such as `led,api`; `web` implies `api`.
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
        run.api |= run.web; // the web display reads from the api
        if !(run.led || run.api) {
            return Err("nothing to run".into());
        }
        Ok(run)
    }

    /// The enabled components' names, for display.
    pub fn names(self) -> Vec<&'static str> {
        [(self.led, "led"), (self.api, "api"), (self.web, "web")]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| n)
            .collect()
    }
}

/// How the HTTP server (api and web) listens.
#[derive(Clone)]
pub struct Server {
    pub bind: String,
    pub port: u16,
    /// Serve the web display from here instead of the copy built into the binary.
    pub web_dir: Option<PathBuf>,
    /// If set, changing settings through the API needs this as a bearer token.
    pub api_token: Option<String>,
    pub max_connections: usize,
}

impl Default for Server {
    fn default() -> Server {
        Server { bind: "127.0.0.1".into(), port: 8080, web_dir: None, api_token: None, max_connections: 32 }
    }
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
    /// Overwrite whichever server options this patch sets.
    fn apply(self, s: &mut Server) {
        if let Some(v) = self.bind {
            s.bind = v;
        }
        if let Some(v) = self.port {
            s.port = v;
        }
        if self.web_dir.is_some() {
            s.web_dir = self.web_dir;
        }
        if self.api_token.is_some() {
            s.api_token = self.api_token;
        }
        if let Some(v) = self.max_connections {
            s.max_connections = v;
        }
    }
}

/// Everything the program is configured with once the file and flags are read.
pub struct Config {
    pub run: Run,
    pub server: Server,
    pub settings: Settings,
    pub custom_groups: BTreeMap<String, GroupDef>,
    /// The config file that was used, if any.
    pub path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            run: Run { led: true, api: false, web: false },
            server: Server::default(),
            settings: Settings::default(),
            custom_groups: BTreeMap::new(),
            path: None,
        }
    }
}

/// What the command line asked for.
pub enum Command {
    Run(Config),
    ListParks,
    ListGroups(Config),
    PrintConfig(Config),
    Help,
}

/// The command line, parsed but not yet combined with the config file.
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
            "-r" | "--run" => {
                a.run = Some(value("--run")?.split(',').map(|s| s.trim().to_string()).collect())
            }
            "-c" | "--config" => a.config = Some(value("--config")?.into()),
            "-s" | "--set" => {
                let kv = value("--set")?;
                let (k, v) =
                    kv.split_once('=').ok_or_else(|| format!("--set needs KEY=VALUE, not {kv:?}"))?;
                a.sets.push((k.trim().to_string(), v.to_string()));
            }
            "-S" | "--show" => a.sets.push(("show".into(), value("--show")?)),
            "-a" | "--all" => a.sets.push(("show_closed".into(), "true".into())),
            "-i" | "--interval" => a.sets.push(("interval".into(), value("--interval")?)),
            "-b" | "--bind" => a.server.bind = Some(value("--bind")?),
            "-p" | "--port" => {
                a.server.port =
                    Some(value("--port")?.parse().map_err(|_| "--port needs a number".to_string())?)
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

/// Where to look for a config file when none is given, most specific first.
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

/// Decode a TOML value, naming the section in any error.
fn section<T: DeserializeOwned>(value: toml::Value, name: &str) -> Result<T, String> {
    value.try_into().map_err(|e: toml::de::Error| format!("{name}: {e}"))
}

/// Apply the contents of a config file to `cfg`. Unknown keys are errors, so typos are caught.
fn apply_file(cfg: &mut Config, text: &str, run: &mut Option<Vec<String>>) -> Result<(), String> {
    let mut table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    if let Some(v) = table.remove("run") {
        *run = Some(section(v, "run")?);
    }
    if let Some(v) = table.remove("server") {
        section::<ServerPatch>(v, "[server]")?.apply(&mut cfg.server);
    }
    if let Some(v) = table.remove("groups") {
        cfg.custom_groups = section(v, "[groups]")?;
    }
    // Whatever is left must be settings.
    let patch: Patch = section(toml::Value::Table(table), "settings")?;
    cfg.settings.apply(patch)
}

/// Read and apply the config file at `path`.
fn load_file(cfg: &mut Config, path: PathBuf, run: &mut Option<Vec<String>>) -> Result<(), String> {
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    apply_file(cfg, &text, run).map_err(|e| format!("{}: {e}", path.display()))?;
    cfg.path = Some(path);
    Ok(())
}

/// Turn the command line into a `Command`, layering defaults, the config file and the flags.
pub fn load(raw: Vec<String>) -> Result<Command, String> {
    let args = parse_args(raw)?;
    if args.help {
        return Ok(Command::Help);
    }
    if args.list_parks {
        return Ok(Command::ListParks);
    }

    let mut cfg = Config::default();
    let mut run = None;
    match args.config {
        Some(path) => load_file(&mut cfg, path, &mut run)?,
        None => {
            if let Some(path) = config_candidates().into_iter().find(|p| p.exists()) {
                load_file(&mut cfg, path, &mut run)?;
            }
        }
    }

    // Command-line flags win over the file.
    run = args.run.or(run);
    if let Some(list) = run {
        cfg.run = Run::parse(&list)?;
    }
    args.server.apply(&mut cfg.server);
    cfg.settings.apply(Patch::from_pairs(&args.sets)?)?;
    if !args.names.is_empty() {
        let directory = queue::directory_or_default();
        let slugs = args.names.iter().map(|n| queue::find_park(&directory, n).map(|p| p.slug));
        let parks = slugs.collect::<Result<Vec<_>, _>>()?;
        cfg.settings.apply(Patch { parks: Some(parks), ..Patch::default() })?;
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
    out.push_str(&format!(
        "bind = {:?}\nport = {}\nmax_connections = {}\n",
        cfg.server.bind, cfg.server.port, cfg.server.max_connections
    ));
    if let Some(d) = &cfg.server.web_dir {
        out.push_str(&format!("web_dir = {:?}\n", d.display().to_string()));
    }
    out.push_str(&format!(
        "api_token = {}\n",
        if cfg.server.api_token.is_some() { "\"(set)\"" } else { "\"\"  # none: API changes are open" }
    ));
    out
}

#[cfg(test)]
mod tests;
