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

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    process::exit(2);
}

fn groups_for(cfg: &Config) -> Groups {
    let mut groups = Groups::builtin();
    if let Err(e) = groups.add_custom(cfg.custom_groups.clone()) {
        fail(&e);
    }
    groups
}

fn run(cfg: Config) {
    let groups = groups_for(&cfg);
    // Fail early on a typo in `show` rather than quietly showing nothing.
    if let Err(e) = groups.compile(&cfg.settings.show) {
        fail(&e);
    }
    let shared = store::Shared::new(cfg.settings.clone(), groups);
    store::spawn_poller(shared.clone());

    let serving = cfg.run.api || cfg.run.web;
    let listener = serving.then(|| {
        web::bind(&cfg.server).unwrap_or_else(|e| fail(&format!("can't listen on {}:{}: {e}", cfg.server.bind, cfg.server.port)))
    });
    if serving {
        let parts: Vec<&str> = [(cfg.run.api, "api"), (cfg.run.web, "web")].into_iter().filter(|(on, _)| *on).map(|(_, n)| n).collect();
        eprintln!("coaster-queue {} on http://{}:{}", parts.join("+"), cfg.server.bind, cfg.server.port);
    }

    let (server, run_cfg) = (cfg.server, cfg.run);
    match (listener, run_cfg.led) {
        (Some(l), true) => {
            let shared2 = shared.clone();
            thread::spawn(move || web::serve(l, shared2, server, run_cfg));
            led::run(shared);
        }
        (Some(l), false) => web::serve(l, shared, server, run_cfg),
        (None, _) => led::run(shared),
    }
}

fn main() {
    let command = config::load(env::args().skip(1).collect()).unwrap_or_else(|e| fail(&e));
    match command {
        Command::Help => print!("{}", config::HELP),
        Command::ListParks => {
            for p in config::directory() {
                println!("{:<32} {}", p.slug, p.name);
            }
        }
        Command::ListGroups(cfg) => {
            let groups = groups_for(&cfg);
            for g in groups.list() {
                println!("{:<14} {:<20} {}", g.id, g.label, if g.builtin { "built-in" } else { "custom" });
            }
        }
        Command::PrintConfig(cfg) => print!("{}", config::print(&cfg)),
        Command::Run(cfg) => run(cfg),
    }
}
