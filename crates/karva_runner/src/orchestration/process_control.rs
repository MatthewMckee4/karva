//! Platform-specific worker process control.
//!
//! Unix workers run in their own process groups, while Windows workers run in
//! Job Objects, so cancellation and crash cleanup cover descendants.

#[cfg(unix)]
mod unix {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, ChildStderr, ChildStdout, Command};

    pub type WorkerChild = Child;

    pub fn spawn(mut command: Command) -> io::Result<WorkerChild> {
        command.spawn()
    }

    pub fn take_stdout(child: &mut WorkerChild) -> Option<ChildStdout> {
        child.stdout.take()
    }

    pub fn take_stderr(child: &mut WorkerChild) -> Option<ChildStderr> {
        child.stderr.take()
    }

    pub fn configure_worker_command(command: &mut Command) {
        command.process_group(0);
    }

    pub fn terminate(child: &Child) -> io::Result<()> {
        signal_process_group(child.id(), libc::SIGTERM)
    }

    /// Kills the worker's process group while the caller retains its leader.
    ///
    /// Callers must not reap `child` before this call. Keeping the leader
    /// waitable prevents its process-group id from being recycled.
    pub fn force_kill(child: &Child) -> io::Result<()> {
        signal_process_group(child.id(), libc::SIGKILL)
    }

    /// Observes worker exit without reaping its process-group leader.
    pub fn has_exited(child: &Child) -> io::Result<bool> {
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        let process_id = libc::id_t::try_from(child.id()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("process id {} cannot be represented as id_t", child.id()),
            )
        })?;
        loop {
            #[expect(
                unsafe_code,
                reason = "observing Unix child exit without reaping requires libc::waitid"
            )]
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    process_id,
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        #[expect(unsafe_code, reason = "successful libc::waitid initializes siginfo_t")]
        let info = unsafe { info.assume_init() };
        #[expect(
            unsafe_code,
            reason = "reading the process id from libc::siginfo_t requires its accessor"
        )]
        let process_id = unsafe { info.si_pid() };
        Ok(process_id != 0)
    }

    fn signal_process_group(process_id: u32, signal: libc::c_int) -> io::Result<()> {
        let process_group_id = process_group_id(process_id)?;
        #[expect(
            unsafe_code,
            reason = "signalling Unix process groups requires libc::kill"
        )]
        let result = unsafe { libc::kill(-process_group_id, signal) };
        if result == 0 {
            return Ok(());
        }

        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(err)
        }
    }

    fn process_group_id(process_id: u32) -> io::Result<libc::pid_t> {
        libc::pid_t::try_from(process_id).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("process id {process_id} cannot be represented as pid_t"),
            )
        })
    }
}

#[cfg(unix)]
pub use unix::*;

#[cfg(not(unix))]
mod windows {
    use std::io;
    use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus};
    use std::thread;

    use crate::orchestration::WORKER_POLL_INTERVAL;
    use subc_jobobject::ContainedChild;

    /// Worker leader and its owning Job Object, assigned before the leader resumes.
    ///
    /// Keeping both handles in one value ensures dropping a worker closes the
    /// kill-on-close Job Object instead of orphaning descendants.
    pub type WorkerChild = ContainedChild<Child>;

    pub fn configure_worker_command(_command: &mut Command) {}

    /// Creates the worker suspended, assigns it to a kill-on-close Job Object,
    /// and resumes it only after assignment succeeds.
    pub fn spawn(mut command: Command) -> io::Result<WorkerChild> {
        subc_jobobject::spawn_contained(&mut command)
    }

    pub fn take_stdout(child: &mut WorkerChild) -> Option<ChildStdout> {
        child.child.stdout.take()
    }

    pub fn take_stderr(child: &mut WorkerChild) -> Option<ChildStderr> {
        child.child.stderr.take()
    }

    /// Stops only the worker so its descendants can finish during the grace period.
    pub fn terminate(child: &mut WorkerChild) -> io::Result<()> {
        child.child.kill()
    }

    /// Terminates the worker's Job Object and all descendants.
    pub fn force_kill(child: &mut WorkerChild) -> io::Result<()> {
        child.job.terminate()
    }

    /// Polls only the leader; callers retain the Job Object until force cleanup.
    pub fn try_wait(child: &mut WorkerChild) -> io::Result<Option<ExitStatus>> {
        child.child.try_wait()
    }

    /// Reaps the leader and waits until the Job Object has no active members.
    pub fn wait(child: &mut WorkerChild) -> io::Result<ExitStatus> {
        let status = child.child.wait()?;
        while child.job.process_count()? != 0 {
            thread::sleep(WORKER_POLL_INTERVAL);
        }
        Ok(status)
    }
}

#[cfg(not(unix))]
pub use windows::*;
