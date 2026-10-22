//! The real binary, run as a subprocess: exit codes, output, and how the command line and
//! config file combine. Each run gets an empty working directory and home so a config file
//! on the machine running the tests can't leak in, and none of them pass a park name (that
//! would fetch the park list from the network).

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

/// An empty directory for one test, removed afterwards.
struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let dir = std::env::temp_dir().join(format!("coaster-queue-cli-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Sandbox(dir)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_coaster-queue"))
            .args(args)
            .current_dir(&self.0)
            .env("HOME", &self.0)
            .env("XDG_CONFIG_HOME", self.0.join("xdg"))
            .output()
            .expect("run the binary")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn path_str(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn help() {
    let o = Sandbox::new("help").run(&["--help"]);
    assert!(o.status.success());
    assert!(stdout(&o).starts_with("coaster-queue - live ride wait times"));
}

#[test]
fn bad_usage_exits_2_with_a_message_on_stderr() {
    let sb = Sandbox::new("usage");
    for (args, mentions) in [
        (&["--nope"][..], "unknown option"),
        (&["--port", "high"], "--port needs a number"),
        (&["--run"], "needs a value"),
        (&["--run", "tv"], "unknown component"),
        (&["--set", "dwell"], "KEY=VALUE"),
        (&["--set", "dwell=1", "--print-config"], "dwell must be between"),
        (&["--set", "nope=1", "--print-config"], "unknown field"),
        (&["--config", "missing.toml"], "missing.toml"),
        // Only checked once the groups are compiled, which happens when running.
        (&["--show", "nonsense", "-r", "api"], "unknown group"),
    ] {
        let o = sb.run(args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", stderr(&o));
        assert!(stderr(&o).contains(mentions), "{args:?}: {}", stderr(&o));
    }
}

#[test]
fn list_groups_shows_built_ins_and_custom_groups() {
    let sb = Sandbox::new("groups");
    let cfg = sb.write("c.toml", "[groups.mine]\nlabel = \"My rides\"\nrides = [\"Nemesis\"]\n");
    let o = sb.run(&["--list-groups", "-c", path_str(&cfg)]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    let line = |id: &str| out.lines().find(|l| l.starts_with(id)).unwrap_or_else(|| panic!("{id} in {out}"));
    assert!(line("thrill").ends_with("built-in"));
    assert!(line("mine").contains("My rides") && line("mine").ends_with("custom"));
}

#[test]
fn a_custom_group_can_override_a_built_in() {
    let sb = Sandbox::new("override");
    let cfg = sb.write("c.toml", "[groups.thrill]\nrides = [\"Nemesis\"]\n");
    let out = stdout(&sb.run(&["-g", "-c", path_str(&cfg)]));
    assert!(out.lines().any(|l| l.starts_with("thrill") && l.ends_with("custom")), "{out}");
}

#[test]
fn print_config_with_no_file_shows_the_defaults() {
    let o = Sandbox::new("defaults").run(&["--print-config"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.starts_with("# no config file found"), "{out}");
    assert!(out.contains("run = [\"led\"]"));
    assert!(out.contains("parks = [\"alton-towers\", \"thorpe-park\"]"));
    assert!(out.contains("port = 8080") && out.contains("bind = \"127.0.0.1\""));
}

/// What `--print-config` prints should itself be a valid config file.
#[test]
fn print_config_output_parses_as_toml() {
    let out = stdout(&Sandbox::new("toml").run(&["--print-config", "-s", "theme=ice"]));
    let table: toml::Table = out.parse().unwrap_or_else(|e| panic!("{e}\n{out}"));
    assert_eq!(table["theme"].as_str(), Some("ice"));
    assert_eq!(table["server"]["port"].as_integer(), Some(8080));
}

#[test]
fn flags_override_the_config_file() {
    let sb = Sandbox::new("layers");
    let cfg = sb.write(
        "c.toml",
        "run = [\"api\"]\ndwell = 5\ntheme = \"green\"\n[server]\nport = 9000\napi_token = \"hunter2\"\n",
    );
    let c = path_str(&cfg);

    let from_file = stdout(&sb.run(&["--print-config", "-c", c]));
    assert!(from_file.contains(&format!("# config file: {c}")));
    assert!(from_file.contains("run = [\"api\"]") && from_file.contains("dwell = 5"));
    assert!(from_file.contains("port = 9000") && from_file.contains("theme = \"green\""));

    let flagged =
        stdout(&sb.run(&["--print-config", "-c", c, "-r", "led", "-p", "9100", "-s", "dwell=30", "-a"]));
    assert!(flagged.contains("run = [\"led\"]") && flagged.contains("port = 9100"), "{flagged}");
    assert!(flagged.contains("dwell = 30") && flagged.contains("show_closed = true"), "{flagged}");
    assert!(flagged.contains("theme = \"green\""), "settings the flags don't mention are kept");
}

/// Regression: --print-config must not reveal the API token.
#[test]
fn print_config_hides_the_api_token() {
    let sb = Sandbox::new("token");
    let out = stdout(&sb.run(&["--print-config", "--api-token", "hunter2"]));
    assert!(!out.contains("hunter2"), "{out}");
    assert!(out.contains("api_token = \"(set)\""));
}

#[test]
fn serve_shorthand_runs_api_and_web() {
    let out = stdout(&Sandbox::new("serve").run(&["serve", "--print-config"]));
    assert!(out.contains("run = [\"api\", \"web\"]"), "{out}");
}

#[test]
fn the_config_file_is_found_in_the_working_directory() {
    let sb = Sandbox::new("discover");
    sb.write("coaster-queue.toml", "dwell = 7\n");
    let out = stdout(&sb.run(&["--print-config"]));
    assert!(out.contains("dwell = 7") && out.contains("coaster-queue.toml"), "{out}");
}

#[test]
fn config_file_mistakes_are_reported_with_the_file_name() {
    let sb = Sandbox::new("bad-file");
    for (text, mentions) in [
        ("dwel = 5\n", "dwel"),
        ("dwell = 1\n", "dwell must be between"),
        ("theme = \"neon\"\n", "theme must be one of"),
        ("[server]\nport = \"x\"\n", "[server]"),
        ("[groups.x]\nride = [\"a\"]\n", "[groups]"),
        ("not toml at all\n", "expected"),
    ] {
        let cfg = sb.write("bad.toml", text);
        let o = sb.run(&["--print-config", "-c", path_str(&cfg)]);
        assert_eq!(o.status.code(), Some(2), "{text}");
        let err = stderr(&o);
        assert!(err.contains("bad.toml") && err.contains(mentions), "{text}: {err}");
    }
}

/// `run` is checked after the file is applied, so its error doesn't name the file.
#[test]
fn a_bad_run_list_in_the_config_file_is_reported() {
    let sb = Sandbox::new("bad-run");
    let cfg = sb.write("c.toml", "run = [\"tv\"]\n");
    let o = sb.run(&["--print-config", "-c", path_str(&cfg)]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("unknown component"), "{}", stderr(&o));
}

#[test]
fn a_bad_custom_group_name_stops_startup() {
    let sb = Sandbox::new("bad-group");
    let cfg = sb.write("c.toml", "[groups.\"Bad Name\"]\nrides = [\"x\"]\n");
    let o = sb.run(&["-g", "-c", path_str(&cfg)]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("isn't valid"), "{}", stderr(&o));
}
