use serde::Deserialize;
use std::path::PathBuf;

fn yes() -> bool {
    true
}

#[derive(Deserialize, Debug)]
pub struct ModSpec {
    pub name: String,
    pub entrypoint: PathBuf,
}

#[derive(Deserialize, Debug)]
pub struct RunnerSpec {
    pub path: Vec<PathBuf>,
    pub launcher: Vec<LaunchSpec>,
    #[serde(default)]
    pub watcher: WatcherSpec,
}

#[derive(Deserialize, Debug)]
pub struct LaunchSpec {
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "yes")]
    pub recurse: bool,
}

#[derive(Deserialize, Debug, Default)]
pub struct WatcherSpec {
    #[serde(default)]
    pub child: Vec<ChildWatcherSpec>,
}

#[derive(Deserialize, Debug, Default)]
pub struct ChildWatcherSpec {
    pub executable: String,
    pub mods: Vec<String>,
}
