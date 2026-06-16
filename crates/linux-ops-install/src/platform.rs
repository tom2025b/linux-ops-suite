//! The host platform the installer targets, and the OS/arch tokens used to
//! match release assets.

use std::env;

use crate::error::InstallError;

#[derive(Debug)]
pub(crate) struct Platform {
    pub(crate) os: &'static str,
    pub(crate) arch: &'static str,
    pub(crate) arch_aliases: &'static [&'static str],
}

impl Platform {
    pub(crate) fn current() -> Result<Self, InstallError> {
        if env::consts::OS != "linux" {
            return Err(InstallError::UnsupportedPlatform(format!(
                "{}-{}",
                env::consts::OS,
                env::consts::ARCH
            )));
        }

        let (arch, arch_aliases) = match env::consts::ARCH {
            "x86_64" => ("x86_64", &["x86_64", "amd64"][..]),
            "aarch64" => ("aarch64", &["aarch64", "arm64"][..]),
            other => return Err(InstallError::UnsupportedPlatform(format!("linux-{other}"))),
        };

        Ok(Self {
            os: "linux",
            arch,
            arch_aliases,
        })
    }

    pub(crate) fn asset_hint(&self) -> String {
        format!("{}-{}", self.os, self.arch)
    }
}
