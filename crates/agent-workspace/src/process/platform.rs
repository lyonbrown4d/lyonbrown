use super::{CommandPolicy, CommandRequest, ResolvedCommand};
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{io, process::Stdio};

pub(super) fn spawn(
  policy: &CommandPolicy,
  request: &CommandRequest,
  resolved: &ResolvedCommand,
) -> io::Result<ProcessGuard> {
  if !cfg!(any(unix, windows)) {
    return Err(io::Error::new(
      io::ErrorKind::Unsupported,
      "process tree cleanup unavailable",
    ));
  }
  let mut command = CommandWrap::with_new(&resolved.executable, |command| {
    command
      .args(&request.arguments)
      .current_dir(&resolved.cwd)
      .env_clear()
      .envs(&policy.environment)
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped());
  });
  command.wrap(KillOnDrop);
  #[cfg(unix)]
  command.wrap(process_wrap::tokio::ProcessGroup::leader());
  #[cfg(windows)]
  command.wrap(process_wrap::tokio::JobObject);
  Ok(ProcessGuard {
    child: command.spawn()?,
    armed: true,
  })
}

pub(super) struct ProcessGuard {
  pub child: Box<dyn ChildWrapper>,
  armed: bool,
}
impl ProcessGuard {
  pub fn stop_tree(&mut self) -> io::Result<()> {
    if !self.armed {
      return Ok(());
    }
    let result = match self.child.start_kill() {
      // Unix ESRCH means the group has already exited.
      #[cfg(unix)]
      Err(error) if error.raw_os_error() == Some(3) => Ok(()),
      result => result,
    };
    if result.is_ok() {
      self.armed = false;
    }
    result
  }
}
impl Drop for ProcessGuard {
  fn drop(&mut self) {
    // Synchronous group/job termination also runs if the caller drops the execution future.
    let _ = self.stop_tree();
  }
}
