use std::fs;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::{
    error::Result,
    health::report::{HealthCheckOutcome, HealthReport, HealthStatus},
    manifest::{HealthCheck, HealthCheckType, ToolManifest},
    paths::{display_relative_to_manifest, expand_manifest_path},
};

/// Execute all health checks declared by a manifest.
pub fn run_health_checks(manifest: &ToolManifest) -> Result<HealthReport> {
    let mut outcomes = Vec::with_capacity(manifest.health.checks.len());

    for check in &manifest.health.checks {
        outcomes.push(run_health_check(manifest, check)?);
    }

    Ok(HealthReport {
        tool_id: manifest.identity.id.clone(),
        outcomes,
    })
}

fn run_health_check(manifest: &ToolManifest, check: &HealthCheck) -> Result<HealthCheckOutcome> {
    let resolved_path = expand_manifest_path(&check.path)?;
    let display_path = display_relative_to_manifest(&resolved_path, manifest);

    let (status, message) = match check.check_type {
        HealthCheckType::FileExists => check_file_exists(&resolved_path, &display_path),
        HealthCheckType::Executable => check_executable(&resolved_path, &display_path),
    };

    Ok(HealthCheckOutcome {
        id: check.id.clone(),
        check_type: check.check_type.clone(),
        path: check.path.clone(),
        resolved_path,
        status,
        message,
    })
}

fn check_file_exists(path: &std::path::Path, display_path: &str) -> (HealthStatus, String) {
    if path.exists() {
        (
            HealthStatus::Passed,
            format!("file exists at {display_path}"),
        )
    } else {
        (
            HealthStatus::Failed,
            format!("file does not exist at {display_path}"),
        )
    }
}

fn check_executable(path: &std::path::Path, display_path: &str) -> (HealthStatus, String) {
    match fs::metadata(path) {
        Ok(metadata) if is_executable(&metadata) => (
            HealthStatus::Passed,
            format!("file is executable at {display_path}"),
        ),
        Ok(_) => (
            HealthStatus::Failed,
            format!("file is not executable at {display_path}"),
        ),
        Err(error) => (
            HealthStatus::Failed,
            format!("cannot inspect {display_path}: {error}"),
        ),
    }
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(metadata: &fs::Metadata) -> bool {
    metadata.is_file()
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use crate::{manifest::load_manifest, test_support::ManifestFixture};

    use super::*;

    #[test]
    fn reports_declared_checks() {
        let workspace = TempWorkspace::new();
        let executable = workspace.path.join("backup-home");
        File::create(&executable).expect("fixture file should be created");

        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&executable)
                .expect("fixture metadata should exist")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&executable, permissions)
                .expect("fixture permissions should be set");
        }

        let manifest_path = workspace.write_manifest(&executable);
        let manifest = load_manifest(&manifest_path).expect("fixture manifest should load");
        let report = run_health_checks(&manifest).expect("health checks should run");

        assert!(report.is_healthy());
        assert_eq!(report.passed_count(), 2);
    }

    #[test]
    fn reports_missing_files_as_failed_checks() {
        let workspace = TempWorkspace::new();
        let executable = workspace.path.join("missing");
        let manifest_path = workspace.write_manifest(&executable);
        let manifest = load_manifest(&manifest_path).expect("fixture manifest should load");
        let report = run_health_checks(&manifest).expect("health checks should run");

        assert!(!report.is_healthy());
        assert_eq!(report.passed_count(), 0);
    }

    struct TempWorkspace {
        path: PathBuf,
    }

    impl TempWorkspace {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("toolfoundry-health-{nonce}"));
            fs::create_dir_all(&path).expect("temp workspace should be created");

            Self { path }
        }

        fn write_manifest(&self, executable: &std::path::Path) -> PathBuf {
            let manifest_path = self.path.join("tool.yaml");
            let manifest = ManifestFixture::with_paths(
                self.path.display(),
                executable.display(),
                executable.display(),
            )
            .with_executable_check(executable.display())
            .yaml();

            fs::write(&manifest_path, manifest).expect("fixture manifest should be written");
            manifest_path
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
