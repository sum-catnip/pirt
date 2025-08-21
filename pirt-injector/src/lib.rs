use std::{
    io,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

use dll_syringe::{Syringe, error::InjectError, process::OwnedProcess};
use glob::{Pattern, glob};
use pirt_common::{ChildWatcherSpec, LaunchSpec, WatcherSpec, ipc::PirtIpcServer};
use thiserror::Error;
use windows::Win32::System::Threading::{CREATE_SUSPENDED, CreateProcessW};

#[derive(Clone, Copy, Debug)]
pub enum Architecture {
    X86,
    X86_64,
}

#[derive(Error, Debug)]
pub enum AttachError {
    #[error("local process io error when injecting into target process")]
    LocalIO(io::Error),
    #[error("io error in remote proccess during injection")]
    RemoteIO(io::Error),
    #[error("target process no longer accessible on injection")]
    TargetInaccessible,
    #[error("target process threw exception on module init")]
    ModuleError(u32),
    #[error("unsupported target. reason: {0}")]
    Unsupported(String),
}

pub fn attach_running(pid: u32, module: &Path) -> Result<(), AttachError> {
    let proc = OwnedProcess::from_pid(pid).map_err(|e| AttachError::LocalIO(e))?;
    let syringe = Syringe::for_process(proc);

    if let Err(e) = syringe.find_or_inject(module) {
        return Err(match e {
            InjectError::Io(io) => AttachError::LocalIO(io),
            InjectError::RemoteIo(io) => AttachError::RemoteIO(io),
            InjectError::IllegalPath(_) => AttachError::LocalIO(io::ErrorKind::InvalidData.into()),
            InjectError::UnsupportedTarget => AttachError::Unsupported("unspecified".into()),
            InjectError::ArchitectureMismatch => {
                AttachError::Unsupported("architecture mismatch".into())
            }
            InjectError::RemoteException(ex) => AttachError::ModuleError(ex.code()),
            InjectError::ProcessInaccessible => AttachError::TargetInaccessible,
            InjectError::Goblin(gob) => AttachError::Unsupported(gob.to_string()),
        });
    };

    Ok(())
}

pub struct Pirt {
    server: PirtIpcServer,
    child_watchers: Vec<ChildWatcher>,
    pirt: PathBuf,
}

impl Pirt {
    pub fn new() -> Self {
        Self {
            server: PirtIpcServer::start(Path::new(r"\\.\pipe\pirt")),
            child_watchers: Vec::new(),
            pirt: PathBuf::from_str("pirt.dll").unwrap(),
        }
    }

    pub fn poll_server(&mut self) {
        for msg in self.server.poll() {
            log::info!("msg: {:?}", msg);
        }
    }

    pub fn launch(&self, spec: LaunchSpec) {
        // launch and give the watchers time to attach
        let cmd = Command::new(&spec.executable)
            .args(spec.args)
            .creation_flags(CREATE_SUSPENDED.0)
            .spawn()
            .unwrap();

        self.notify_child_watchers(cmd.id(), spec.executable.as_path());
    }

    pub fn add_child_watcher(&mut self, spec: ChildWatcherSpec) {
        self.child_watchers.push(ChildWatcher { spec });
    }

    fn notify_child_watchers(&self, pid: u32, path: &Path) {
        for w in &self.child_watchers {
            if w.notify(path) {
                let Ok(_) = attach_running(pid, &self.pirt).inspect_err(|e| {
                    log::error!("error attaching to child process [{}]. reason: {}", pid, e);
                }) else {
                    continue;
                };
            }
        }
    }
}

struct ChildWatcher {
    spec: ChildWatcherSpec,
}

impl ChildWatcher {
    pub fn notify(&self, path: &Path) -> bool {
        let Ok(pattern) = Pattern::new(&self.spec.glob)
            .inspect_err(|e| log::error!("invalid glob pattern: {}", e))
        else {
            return false;
        };

        if !pattern.matches_path(path) {
            log::debug!(
                "path {:?} doesnt match pattern {:?}, skipping",
                path,
                self.spec.glob
            );

            return false;
        };

        return true;
    }
}
