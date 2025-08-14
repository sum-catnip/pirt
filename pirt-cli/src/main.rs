use anyhow::Result;
use clap::{Parser, Subcommand};
use glob::glob;
use log::info;
use pirt_common::{ChildWatcherSpec, LaunchSpec, ModSpec, RunnerSpec, WatcherSpec};
use pirt_injector::attach_running;
use serde::Deserialize;
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
use toml::Table;

#[derive(Parser)]
struct Cli {
    config: PathBuf,
}

fn main() -> Result<()> {
    env_logger::init();

    let cli = Cli::parse();

    let cwd = std::env::current_dir()?;
    let workdir = cli.config.parent().unwrap_or(&cwd);
    log::debug!("workdir: {:?}", workdir);

    log::debug!("loading runner spec");
    let runnerspec = &fs::read_to_string(&cli.config)?;
    let runnerspec: RunnerSpec = toml::from_str(&runnerspec)?;
    log::debug!("runnerspec: {:?}", runnerspec);

    std::env::set_current_dir(workdir)?;

    log::info!("collecting module specs from pirt path");
    let mut specs = Vec::new();
    for p in runnerspec.path {
        specs.append(&mut collect_modules(&p));
    }

    log::debug!("modspecs: {:?}", specs);

    Ok(())
}

fn collect_modules(p: &Path) -> Vec<ModSpec> {
    std::env::set_current_dir(p).unwrap();
    let mut specs = Vec::new();
    let Ok(globiter) = glob("**/*.pirt.toml") else {
        log::error!("error globing {:?} for modspecs", p);
        return specs;
    };

    for pirtmod in globiter {
        log::debug!("found: {:?}", pirtmod);
        let Ok(p) = pirtmod else {
            log::error!(
                "error collecting modspecs from dir: {}",
                pirtmod.err().unwrap()
            );
            continue;
        };

        let Ok(modstr) = fs::read_to_string(&p) else {
            log::error!("error reading modspec file: {:?}", &p);
            continue;
        };

        let Ok(modspec) = toml::from_str::<ModSpec>(&modstr)
            .inspect_err(|e| log::error!("malformed modspec file: {:?}. error: {}", &p, e))
        else {
            continue;
        };

        specs.push(modspec);
    }

    specs
}
