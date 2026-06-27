use std::fmt::Display;

pub struct ManifestFixture {
    id: String,
    display_name: String,
    summary: String,
    tags: Vec<String>,
    project: String,
    repo: String,
    criticality: String,
    local_path: String,
    primary_file: String,
    artifact_path: String,
    target_path: String,
    link_source: String,
    link_target: String,
    health_checks: Vec<HealthCheckFixture>,
    requires_sudo: bool,
    // Lifecycle state + review date are now overridable so tests can exercise the
    // attention rule (P1-2: broken/risky => attention) and review-due derivation.
    // We store them as strings to keep the fixture decoupled from the enum type.
    lifecycle_state: String,
    review_after: String,
}

struct HealthCheckFixture {
    id: String,
    check_type: String,
    path: String,
}

impl ManifestFixture {
    pub fn default_paths() -> Self {
        Self::with_paths(
            "~/projects/backupsage",
            "~/projects/backupsage/bin/backup-home",
            "~/.local/bin/backup-home",
        )
        .with_executable_check("~/projects/backupsage/bin/backup-home")
    }

    pub fn with_paths(
        local_path: impl Display,
        artifact_path: impl Display,
        target_path: impl Display,
    ) -> Self {
        let artifact_path = artifact_path.to_string();
        let target_path = target_path.to_string();

        Self {
            id: "backup-home".to_string(),
            display_name: "Backup Home".to_string(),
            summary: "Back up selected home directories to external storage.".to_string(),
            tags: vec![
                "backup".to_string(),
                "filesystem".to_string(),
                "personal-ops".to_string(),
            ],
            project: "backupsage".to_string(),
            repo: "https://github.com/tom2025b/backupsage".to_string(),
            criticality: "high".to_string(),
            local_path: local_path.to_string(),
            primary_file: artifact_path.clone(),
            artifact_path: artifact_path.clone(),
            target_path: target_path.clone(),
            link_source: artifact_path,
            link_target: target_path.clone(),
            health_checks: vec![HealthCheckFixture {
                id: "target-exists".to_string(),
                check_type: "file_exists".to_string(),
                path: target_path,
            }],
            requires_sudo: false,
            // Defaults preserve the previous hardcoded manifest: an active tool
            // whose review falls due on 2026-09-01. Existing tests are unaffected.
            lifecycle_state: "active".to_string(),
            review_after: "2026-09-01".to_string(),
        }
    }

    pub fn with_identity(
        mut self,
        id: impl Display,
        display_name: impl Display,
        summary: impl Display,
    ) -> Self {
        let id = id.to_string();
        self.id = id.clone();
        self.display_name = display_name.to_string();
        self.summary = summary.to_string();
        self.repo = format!("https://example.invalid/{id}");
        self
    }

    pub fn with_tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|tag| (*tag).to_string()).collect();
        self
    }

    pub fn with_project(mut self, project: impl Display) -> Self {
        self.project = project.to_string();
        self
    }

    pub fn with_criticality(mut self, criticality: impl Display) -> Self {
        self.criticality = criticality.to_string();
        self
    }

    pub fn with_executable_check(mut self, path: impl Display) -> Self {
        self.health_checks.push(HealthCheckFixture {
            id: "executable".to_string(),
            check_type: "executable".to_string(),
            path: path.to_string(),
        });
        self
    }

    pub fn with_requires_sudo(mut self, requires_sudo: bool) -> Self {
        self.requires_sudo = requires_sudo;
        self
    }

    /// Override only the link source/target, leaving `install.artifact_path` and
    /// `install.target_path` pointed at their original values. Lets drift tests
    /// construct a manifest whose desired link is current while the install paths
    /// still refer to the same (validated) source/target.
    pub fn with_link(mut self, source: impl Display, target: impl Display) -> Self {
        self.link_source = source.to_string();
        self.link_target = target.to_string();
        self
    }

    /// Override the lifecycle state (e.g. "broken", "risky", "deprecated").
    /// Used by the attention-rule tests: per P1-2, broken/risky must surface as
    /// `status: attention` while stale/deprecated stay `ok`.
    pub fn with_lifecycle_state(mut self, state: impl Display) -> Self {
        self.lifecycle_state = state.to_string();
        self
    }

    pub fn yaml(&self) -> String {
        format!(
            r#"
schema_version: 1
kind: Tool
identity:
  id: {}
  display_name: {}
  summary: {}
  kind: script
  tags: [{}]
ownership:
  owner: tom
  maintainer: tom
  project: {}
  repo: {}
  local_path: {}
  criticality: {}
source:
  language: bash
  primary_file: {}
  build: none
install:
  method: symlink
  artifact_path: {}
  target_path: {}
  requires_sudo: {}
links:
  managed: true
  desired:
    - source: {}
      target: {}
health:
  checks:
{}lifecycle:
  state: {}
  review_after: "{}"
  replacement: null
"#,
            self.id,
            self.display_name,
            self.summary,
            self.tags.join(", "),
            self.project,
            self.repo,
            self.local_path,
            self.criticality,
            self.primary_file,
            self.artifact_path,
            self.target_path,
            self.requires_sudo,
            self.link_source,
            self.link_target,
            self.render_health_checks(),
            self.lifecycle_state,
            self.review_after
        )
    }

    fn render_health_checks(&self) -> String {
        self.health_checks
            .iter()
            .map(|check| {
                format!(
                    "    - id: {}\n      type: {}\n      path: {}\n",
                    check.id, check.check_type, check.path
                )
            })
            .collect()
    }
}
