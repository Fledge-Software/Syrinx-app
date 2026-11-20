//! Reaps only fixture-owned child processes, with bounded waits and failure cleanup.

use std::{
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) enum ChildOutput {
    Quiet,
    Inherit,
}

pub(super) struct OwnedChild {
    pub(super) child: Option<Child>,
}

impl OwnedChild {
    pub(super) fn spawn(command: &mut Command, output: ChildOutput) -> Result<Self, String> {
        let (stdout, stderr) = match output {
            ChildOutput::Quiet => (Stdio::null(), Stdio::null()),
            ChildOutput::Inherit => (Stdio::inherit(), Stdio::inherit()),
        };
        let child = command
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
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
        let mut child =
            OwnedChild::spawn(Command::new("/bin/sleep").arg("30"), ChildOutput::Quiet).unwrap();
        assert!(child.poll().unwrap().is_none());
        child.stop().unwrap();
        assert!(child.child.is_none());
    }

    #[test]
    fn child_output_policy_preserves_failures_and_keeps_tls_quiet() {
        const MODE: &str = "SYRINX_SYNTHETIC_CHILD_OUTPUT";
        if let Some(mode) = std::env::var_os(MODE) {
            assert!(mode == "inherit" || mode == "quiet");
            let output = if mode == "inherit" {
                ChildOutput::Inherit
            } else {
                ChildOutput::Quiet
            };
            let mut child = OwnedChild::spawn(Command::new("/bin/sh").args([
                "-c", "printf 'synthetic-child-progress\\n'; printf 'synthetic-child-failure-cause\\n' >&2; exit 23",
            ]), output).unwrap();
            assert_eq!(child.wait(Duration::from_secs(5)).unwrap().code(), Some(23));
            assert!(child.child.is_none(), "Failed child must be reaped");
            println!("synthetic-child-reaped");
            return;
        }

        // An isolated test process redirects its inherited descriptors to a private file.
        let directory = crate::linux::fixture::reserve_profile().unwrap();
        for mode in ["inherit", "quiet"] {
            let path = directory.join(mode);
            let output = std::fs::File::create(&path).unwrap();
            let mut runner = OwnedChild { child: Some(Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "linux::child_process::tests::child_output_policy_preserves_failures_and_keeps_tls_quiet", "--nocapture"])
                .env(MODE, mode)
                .stdin(Stdio::null())
                .stdout(output.try_clone().unwrap())
                .stderr(output)
                .spawn().unwrap()) };
            assert!(runner.wait(Duration::from_secs(10)).unwrap().success());
            assert!(runner.child.is_none());
            let observed = std::fs::read_to_string(&path).unwrap();
            assert!(observed.contains("synthetic-child-reaped"));
            for message in ["synthetic-child-progress", "synthetic-child-failure-cause"] {
                assert_eq!(
                    observed.contains(message),
                    mode == "inherit",
                    "Child output policy {mode} lost or leaked {message}"
                );
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
