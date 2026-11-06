//! Reaps only fixture-owned child processes, with bounded waits and failure cleanup.

use std::{
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) struct OwnedChild {
    pub(super) child: Option<Child>,
}

impl OwnedChild {
    pub(super) fn spawn(command: &mut Command) -> Result<Self, String> {
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        Ok(Self { child: Some(child) })
    }

    pub(super) fn poll(&mut self) -> Result<Option<ExitStatus>, String> {
        self.child
            .as_mut()
            .ok_or("Child already reaped")?
            .try_wait()
            .map_err(|error| error.to_string())
    }

    pub(super) fn wait(&mut self, bound: Duration) -> Result<ExitStatus, String> {
        let deadline = Instant::now() + bound;
        while Instant::now() < deadline {
            if let Some(status) = self.poll()? {
                self.child.take();
                return Ok(status);
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err("Owned fixture child did not exit within its deadline".into())
    }

    pub(super) fn stop(&mut self) -> Result<(), String> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none()
        {
            child.kill().map_err(|error| error.to_string())?;
        }
        self.wait(Duration::from_secs(5))?;
        Ok(())
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("Owned child cleanup: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_running_child_is_killed_and_reaped_within_bound() {
        let mut child = OwnedChild::spawn(Command::new("/bin/sleep").arg("30")).unwrap();
        assert!(child.poll().unwrap().is_none());
        child.stop().unwrap();
        assert!(child.child.is_none());
    }
}
