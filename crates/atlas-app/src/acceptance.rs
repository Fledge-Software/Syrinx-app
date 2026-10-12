//! P1-33's palette acceptance run: its options, frame timing probe and machine-readable report.

mod probe;
mod report;

use std::path::PathBuf;

pub use probe::PresentProbe;
pub use report::{KeySample, OpenSample, Report, SizeResult};

/// What `--test-palette-acceptance <report.json> <page base>` asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct AcceptancePlan {
    /// Where the JSON report is written.
    pub report: PathBuf,
    /// The synthetic loopback page server, such as `http://127.0.0.1:50123`.
    pub page_base: String,
}

impl AcceptancePlan {
    /// Accepts only a plain `http://127.0.0.1:<port>` base, so no real site can be named.
    pub fn new(report: PathBuf, page_base: &str) -> Result<Self, &'static str> {
        let port = page_base
            .strip_prefix("http://127.0.0.1:")
            .ok_or("The acceptance pages must come from http://127.0.0.1:<port>.")?;
        let valid = !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|port| port != 0);
        if !valid {
            return Err("The acceptance page port must be a number from 1 to 65535.");
        }
        Ok(Self {
            report,
            page_base: page_base.to_owned(),
        })
    }

    /// The address of one synthetic page; its own script reports focus in its title.
    pub fn page(&self, label: &str) -> String {
        format!("{}/page/{label}", self.page_base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_loopback_page_server_is_accepted() {
        let plan = AcceptancePlan::new(PathBuf::from("report.json"), "http://127.0.0.1:50123")
            .expect("loopback base");
        assert_eq!(
            plan.page("focus-page"),
            "http://127.0.0.1:50123/page/focus-page"
        );
        for base in [
            "https://127.0.0.1:50123",
            "http://localhost:50123",
            "http://127.0.0.1:",
            "http://127.0.0.1:0",
            "http://127.0.0.1:70000",
            "http://127.0.0.1:50123/elsewhere",
            "http://127.0.0.1:50123@example.com",
            "https://www.example.com",
        ] {
            assert!(
                AcceptancePlan::new(PathBuf::from("report.json"), base).is_err(),
                "{base} must be refused"
            );
        }
    }
}
