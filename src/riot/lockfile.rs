//! Locating and parsing the Riot Client lockfile.

use std::path::PathBuf;

use crate::secret::Secret;

/// Parsed `name:pid:port:password:protocol` lockfile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lockfile {
    pub name: String,
    pub pid: u32,
    pub port: u16,
    /// Basic-auth password for the local API. Never log.
    pub password: Secret<String>,
    pub protocol: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LockfileError {
    #[error("expected 5 colon-separated fields, found {0}")]
    FieldCount(usize),
    #[error("pid is not a number")]
    BadPid,
    #[error("port is not a valid port number")]
    BadPort,
    #[error("field {0} is empty")]
    EmptyField(&'static str),
}

impl std::str::FromStr for Lockfile {
    type Err = LockfileError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse(s)
    }
}

/// Parse lockfile contents. Trailing whitespace/newlines are ignored.
pub fn parse(contents: &str) -> Result<Lockfile, LockfileError> {
    let fields: Vec<&str> = contents.trim().split(':').collect();
    if fields.len() != 5 {
        return Err(LockfileError::FieldCount(fields.len()));
    }
    let names = ["name", "pid", "port", "password", "protocol"];
    for (field, name) in fields.iter().zip(names) {
        if field.is_empty() {
            return Err(LockfileError::EmptyField(name));
        }
    }
    let pid = fields[1].parse().map_err(|_| LockfileError::BadPid)?;
    let port: u16 = fields[2].parse().map_err(|_| LockfileError::BadPort)?;
    if port == 0 {
        return Err(LockfileError::BadPort);
    }
    Ok(Lockfile {
        name: fields[0].to_string(),
        pid,
        port,
        password: Secret::new(fields[3].to_string()),
        protocol: fields[4].to_string(),
    })
}

/// Where the lockfile lives. `TRACKED_LOCKFILE` overrides it (for development
/// and tests); otherwise `%LOCALAPPDATA%\Riot Games\Riot Client\Config\lockfile`.
pub fn default_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("TRACKED_LOCKFILE") {
        return Some(PathBuf::from(p));
    }
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.data_local_dir().to_path_buf()))?;
    Some(
        local
            .join("Riot Games")
            .join("Riot Client")
            .join("Config")
            .join("lockfile"),
    )
}

/// Read the lockfile. A missing file means the client isn't running.
pub async fn read() -> Result<Lockfile, super::error::RiotError> {
    use super::error::RiotError;
    let path = default_path().ok_or(RiotError::ClientNotRunning)?;
    // The Riot client keeps the file open; tokio's read uses shared access on
    // Windows so this works while the client is running.
    match tokio::fs::read_to_string(&path).await {
        Ok(s) => Ok(parse(&s)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(RiotError::ClientNotRunning),
        Err(e) => {
            tracing::warn!("couldn't read lockfile: {e}");
            Err(RiotError::LockfileMalformed)
        }
    }
}
