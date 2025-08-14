use std::{io, path::Path};

use dll_syringe::{Syringe, error::InjectError, process::OwnedProcess};
use thiserror::Error;

#[derive(Clone, Copy, Debug)]
pub enum Architecture {
    X86,
    X86_64,
}

#[derive(Error, Debug)]
pub enum AttachError {
    #[error("failed to open process for attaching")]
    CantOpenProcess(#[from] io::Error),
    #[error("local process io error when injecting into target process")]
    LocalIO(#[from] io::Error),
    #[error("io error in remote proccess during injection")]
    RemoteIO(#[from] io::Error),
    #[error("target process no longer accessible on injection")]
    TargetInaccessible,
    #[error("target process threw exception on module init")]
    ModuleError(u32),
    #[error("incompatible architecture mismatch. target is {target:?}, injector is: {injector:?}")]
    ArchitectureMismatch {
        injector: Architecture,
        target: Architecture,
    },
}

pub fn attach_running(pid: u32, module: &Path) -> Result<(), AttachError> {
    let proc = OwnedProcess::from_pid(pid)?;
    let syringe = Syringe::for_process(proc);

    if let Err(e) = syringe.find_or_inject(module) {
        return Err(match e {
            InjectError::Io(io) => AttachError::LocalIO(io),
            InjectError::RemoteIo(io) => AttachError::RemoteIO(io),
        });
    };

    Ok(())
}
