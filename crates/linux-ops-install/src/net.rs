//! Network and subprocess plumbing: a thin `curl` HTTP fetch and the shared
//! command runner every shell-out goes through.

use std::fs;
use std::io;
use std::process::Command;

use crate::error::InstallError;
use crate::fs_ops::TempDir;

pub(crate) fn fetch_http(url: &str) -> Result<(u16, Vec<u8>), InstallError> {
    // Write the body to a temp file and read the HTTP status from curl's own
    // `-w '%{http_code}'` on stdout. This keeps the status out-of-band rather
    // than appending a sentinel to the body — a response body that happened to
    // contain the sentinel string could otherwise be split at the wrong point.
    let temp_dir = TempDir::new("http")?;
    let body_path = temp_dir.path().join("body");

    let stdout = run_command(
        Command::new("curl")
            .arg("-sSL")
            .arg("--max-redirs")
            .arg("10")
            .arg("-H")
            .arg("Accept: application/vnd.github+json")
            .arg("-H")
            .arg("User-Agent: linux-ops-install")
            .arg("-o")
            .arg(&body_path)
            .arg("-w")
            .arg("%{http_code}")
            .arg(url),
    )?;

    let status = String::from_utf8_lossy(&stdout)
        .trim()
        .parse::<u16>()
        .map_err(|source| InstallError::Io {
            context: format!("parse HTTP status for {url}"),
            source: io::Error::new(io::ErrorKind::InvalidData, source),
        })?;

    let body = fs::read(&body_path).map_err(|source| InstallError::Io {
        context: format!("read HTTP response body for {url}"),
        source,
    })?;

    Ok((status, body))
}

pub(crate) fn run_command(command: &mut Command) -> Result<Vec<u8>, InstallError> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command.output().map_err(|source| InstallError::Io {
        context: format!("run {program}"),
        source,
    })?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(InstallError::CommandFailed {
            program,
            status: output
                .status
                .code()
                .map(|code| format!("exit code {code}"))
                .unwrap_or_else(|| "signal".to_string()),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
