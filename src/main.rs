mod config;
mod groups;
mod led;
mod queue;
mod settings;
mod store;
mod web;

use config::{Command, Config};
use groups::Groups;
use std::{env, process, thread};

/// Report a configuration problem and exit.
fn fail(message: &str) -> ! {
    eprintln!("{message}");
    process::exit(2);
}

/// The built-in ride groups plus any custom ones from the config.
fn groups_for(cfg: &Config) -> Groups {
    let mut groups = Groups::builtin();
    if let Err(e) = groups.add_custom(cfg.custom_groups.clone()) {
        fail(&e);
    }
    groups
}

/// Start whichever parts were asked for. The terminal display, if wanted, runs on this
/// thread until the user quits; a server runs on its own thread beside it, or on this
/// one when there's no display.
fn run(cfg: Config) {
    let groups = groups_for(&cfg);
    // Fail early on a typo in `show` rather than quietly showing nothing.
    if let Err(e) = groups.compile(&cfg.settings.show) {
        fail(&e);
    }
    let shared = store::Shared::new(cfg.settings, groups);
    store::spawn_poller(shared.clone());

    let Config { run, server, .. } = cfg;
    // Bind before the display takes over the terminal, so a bad address is reported properly.
    let listener = (run.api || run.web).then(|| {
        let listener = web::bind(&server)
            .unwrap_or_else(|e| fail(&format!("can't listen on {}:{}: {e}", server.bind, server.port)));
        eprintln!("coaster-queue {} on http://{}:{}", run.names().join("+"), server.bind, server.port);
        listener
    });

    match (listener, run.led) {
        (Some(listener), true) => {
            let for_server = shared.clone();
            thread::spawn(move || web::serve(&listener, for_server, server, run));
            led::run(&shared);
        }
        (Some(listener), false) => web::serve(&listener, shared, server, run),
        (None, _) => led::run(&shared),
    }
}

fn main() {
    match config::load(env::args().skip(1).collect()).unwrap_or_else(|e| fail(&e)) {
        Command::Help => print!("{}", config::HELP),
        Command::ListParks => {
            for p in queue::directory_or_default() {
                println!("{:<32} {}", p.slug, p.name);
            }
        }
        Command::ListGroups(cfg) => {
            for g in groups_for(&cfg).list() {
                println!("{:<14} {:<20} {}", g.id, g.label, if g.builtin { "built-in" } else { "custom" });
            }
        }
        Command::PrintConfig(cfg) => print!("{}", config::print(&cfg)),
        Command::Run(cfg) => run(cfg),
    }
}
