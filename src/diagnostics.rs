//! The connection checklist shown on the startup screen.
//!
//! The worker fills this in step by step while it connects to the Riot
//! client, so when something fails the user (and a bug report) can see
//! exactly which step and why. Messages never contain credentials.

use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    Lockfile,
    LocalApi,
    Region,
    ClientVersion,
    Identity,
    RiotServers,
    LiveUpdates,
}

impl Step {
    /// In the order `riot::connect_observed` runs them.
    pub const ALL: [Step; 7] = [
        Step::Lockfile,
        Step::LocalApi,
        Step::Identity,
        Step::Region,
        Step::ClientVersion,
        Step::RiotServers,
        Step::LiveUpdates,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Step::Lockfile => "Riot Client running",
            Step::LocalApi => "Signed in to the Riot Client",
            Step::Region => "Region detected",
            Step::ClientVersion => "Game version",
            Step::Identity => "Riot ID",
            Step::RiotServers => "Riot match servers",
            Step::LiveUpdates => "Live game updates",
        }
    }

    /// Headline for the startup screen when this step is the one failing.
    pub fn failure_headline(self) -> &'static str {
        match self {
            Step::Lockfile => "Waiting for the Riot Client…",
            Step::LocalApi => "The Riot Client isn't ready yet",
            Step::Region => "Waiting for VALORANT…",
            Step::ClientVersion => "Couldn't get the game version",
            Step::Identity => "Couldn't read your Riot ID",
            Step::RiotServers => "Riot's match servers aren't responding",
            Step::LiveUpdates => "Live updates aren't connected",
        }
    }

    /// Steps that must pass before the app counts as connected.
    pub fn required(self) -> bool {
        !matches!(self, Step::Identity | Step::LiveUpdates)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepState {
    Pending,
    Running,
    Ok(String),
    /// Not fatal: the app works, with less information.
    Warn(String),
    Failed {
        message: String,
        hint: String,
    },
}

impl StepState {
    pub fn failed(message: impl Into<String>, hint: impl Into<String>) -> Self {
        StepState::Failed {
            message: message.into(),
            hint: hint.into(),
        }
    }

    fn passed(&self) -> bool {
        matches!(self, StepState::Ok(_) | StepState::Warn(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionReport {
    pub steps: Vec<(Step, StepState)>,
    /// Increments on every connection attempt.
    pub attempt: u32,
}

impl Default for ConnectionReport {
    fn default() -> Self {
        Self {
            steps: Step::ALL.iter().map(|s| (*s, StepState::Pending)).collect(),
            attempt: 0,
        }
    }
}

impl ConnectionReport {
    pub fn state(&self, step: Step) -> &StepState {
        &self
            .steps
            .iter()
            .find(|(s, _)| *s == step)
            .expect("every step is present")
            .1
    }

    pub fn set(&mut self, step: Step, state: StepState) {
        if let Some(slot) = self.steps.iter_mut().find(|(s, _)| *s == step) {
            slot.1 = state;
        }
    }

    /// Start a new attempt: everything back to pending.
    pub fn restart(&mut self) {
        let attempt = self.attempt + 1;
        *self = Self {
            attempt,
            ..Default::default()
        };
    }

    /// All required steps passed.
    pub fn connected(&self) -> bool {
        self.steps
            .iter()
            .filter(|(s, _)| s.required())
            .all(|(_, st)| st.passed())
    }

    /// The first failed step, if any.
    pub fn failure(&self) -> Option<(Step, &str, &str)> {
        self.steps.iter().find_map(|(s, st)| match st {
            StepState::Failed { message, hint } => Some((*s, message.as_str(), hint.as_str())),
            _ => None,
        })
    }

    /// Plain-text report for bug reports ("Copy diagnostics").
    pub fn to_text(&self, log_dir: Option<&std::path::Path>) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "Ascend-Lite v{} on {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        );
        let _ = writeln!(out, "Connection attempt #{}", self.attempt);
        for (step, st) in &self.steps {
            let line = match st {
                StepState::Pending => "pending".to_string(),
                StepState::Running => "running".to_string(),
                StepState::Ok(m) => format!("OK - {m}"),
                StepState::Warn(m) => format!("WARN - {m}"),
                StepState::Failed { message, hint } => format!("FAILED - {message} (hint: {hint})"),
            };
            let _ = writeln!(out, "[{}] {line}", step.title());
        }
        if let Some(dir) = log_dir {
            let _ = writeln!(out, "Logs: {}", dir.display());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_ignores_optional_steps() {
        let mut r = ConnectionReport::default();
        assert!(!r.connected());
        for s in Step::ALL.iter().filter(|s| s.required()) {
            r.set(*s, StepState::Ok("ok".into()));
        }
        assert!(r.connected(), "identity + live updates are optional");
        r.set(Step::Region, StepState::failed("no region", "launch VALORANT"));
        assert!(!r.connected());
        assert_eq!(r.failure().map(|f| f.0), Some(Step::Region));
        let text = r.to_text(None);
        assert!(
            text.contains("[Region detected] FAILED - no region (hint: launch VALORANT)"),
            "{text}"
        );
        r.restart();
        assert_eq!(r.attempt, 1);
        assert_eq!(r.state(Step::Region), &StepState::Pending);
    }
}
