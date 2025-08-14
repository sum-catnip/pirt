use std::{
    io,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};

use dll_syringe::{Syringe, error::InjectError, process::OwnedProcess};
use pirt_common::LaunchSpec;
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

pub struct Pirt {}
impl Pirt {
    pub fn launch(spec: LaunchSpec) {
        // launch and give the watchers time to attach
        let cmd = Command::new(spec.executable)
            .args(spec.args)
            .creation_flags(CREATE_SUSPENDED.0)
            .spawn()
            .unwrap();
    }
}
