//! Writing a session's native state to disk, and finding it again.
//!
//! The core stores an opaque payload it never reads, per RFC-0003. What is in
//! it is the adapter's business; that it lands on disk with the right
//! permissions is this module's.

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use gyr_model::SessionState;

use crate::session::SinkError;

/// Where a session's state lives, beside its log.
#[must_use]
pub fn state_path(workspace: &Path, session_id: &str) -> PathBuf {
    workspace
        .join(".gyr")
        .join("sessions")
        .join(format!("{session_id}.state.json"))
}

/// Writes state atomically, owner-readable only.
///
/// **This file holds the conversation, and the conversation holds your source.**
/// Tool results are in it: file contents, search hits, compiler output, and
/// whatever reasoning the provider returned and the adapter retained. It goes
/// inside the workspace, in a directory git already ignores, with mode 0600 on
/// Unix. RFC-0014 section 4 says so out loud rather than in a footnote.
///
/// # Errors
///
/// Returns [`SinkError`] when the directory cannot be made or the file cannot
/// be written or renamed into place.
pub fn save(path: &Path, state: &SessionState) -> Result<(), SinkError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            SinkError::new(format!(
                "cannot create session state directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    let encoded = serde_json::to_vec_pretty(state)
        .map_err(|error| SinkError::new(format!("cannot encode session state: {error}")))?;

    let temporary = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| SinkError::new(format!("cannot open {}: {error}", temporary.display())))?;
    let written = file
        .write_all(&encoded)
        .and_then(|()| file.sync_all())
        .map_err(|error| SinkError::new(format!("cannot write {}: {error}", temporary.display())));
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        SinkError::new(format!("cannot replace {}: {error}", path.display()))
    })
}

/// Reads state back.
///
/// # Errors
///
/// Returns [`SinkError`] when the file cannot be read or is not a state file.
pub fn load(path: &Path) -> Result<SessionState, SinkError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| SinkError::new(format!("cannot read {}: {error}", path.display())))?;
    serde_json::from_str(&text).map_err(|error| {
        SinkError::new(format!(
            "{} is not a session state: {error}",
            path.display()
        ))
    })
}

/// The most recently modified session state in a workspace, if there is one.
///
/// # Errors
///
/// Returns [`SinkError`] when the sessions directory exists but cannot be read.
pub fn most_recent(workspace: &Path) -> Result<Option<(String, PathBuf)>, SinkError> {
    let directory = workspace.join(".gyr").join("sessions");
    if !directory.is_dir() {
        return Ok(None);
    }
    let entries = std::fs::read_dir(&directory)
        .map_err(|error| SinkError::new(format!("cannot read {}: {error}", directory.display())))?;

    let mut best: Option<(std::time::SystemTime, String, PathBuf)> = None;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(id) = name.strip_suffix(".state.json") else {
            continue;
        };
        let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(when, _, _)| modified > *when) {
            best = Some((modified, id.to_owned(), path));
        }
    }
    Ok(best.map(|(_, id, path)| (id, path)))
}
