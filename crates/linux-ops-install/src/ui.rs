//! Console output helpers and the install banner/mode/path guidance. All
//! user-facing text lives here so the formatting conventions stay in one place.

use crate::path_contains;
use crate::Cli;
use crate::InstallPaths;

pub(crate) fn print_banner() {
    println!();
    println!("==================================================");
    println!(" Linux Ops Suite Installer");
    println!("==================================================");
}

pub(crate) fn print_mode(cli: &Cli) {
    if cli.dry_run {
        println!("Mode : dry run");
    } else {
        println!("Mode : install");
    }

    if cli.force {
        println!("Force: enabled");
    } else {
        println!("Force: disabled");
    }

    // Echo the verification posture — these two flags have the biggest security
    // impact, so the banner must make a downgraded posture impossible to miss.
    if cli.no_verify {
        println!("Verify: DISABLED (--no-verify; downloads installed unchecked)");
    } else if cli.allow_unverified {
        println!("Verify: relaxed (--allow-unverified; missing checksum installs with a warning)");
    } else {
        println!("Verify: enabled (SHA256, fails closed)");
    }
}

pub(crate) fn print_path_guidance(paths: &InstallPaths) {
    println!();
    println!("Done.");
    if !path_contains(&paths.bin_dir) || !path_contains(&paths.wrapper_dir) {
        println!();
        println!("Add this to your shell rc if needed:");
        println!(
            "    export PATH=\"{}:{}:$PATH\"",
            paths.bin_dir.display(),
            paths.wrapper_dir.display()
        );
    }
    println!();
    println!("Source aliases from your shell rc once:");
    println!(
        "    [ -f \"{}\" ] && source \"{}\"",
        paths.aliases_file.display(),
        paths.aliases_file.display()
    );
    println!();
    println!("Then refresh the suite snapshot:");
    println!("    workstate    # compiles the canonical snapshot");
    println!("    (see the README \"Running a full suite refresh\" for the producers -> snapshot -> consumer flow)");
}

pub(crate) fn step(message: impl AsRef<str>) {
    println!("==> {}", message.as_ref());
}

pub(crate) fn ok(message: impl AsRef<str>) {
    println!("  ok {}", message.as_ref());
}

pub(crate) fn skip(message: impl AsRef<str>) {
    println!("  skip {}", message.as_ref());
}

pub(crate) fn warn(message: impl AsRef<str>) {
    println!("  warn {}", message.as_ref());
}

pub(crate) fn detail(message: impl AsRef<str>) {
    println!("       {}", message.as_ref());
}

pub(crate) fn dry_run(message: impl AsRef<str>) {
    println!("  dry-run {}", message.as_ref());
}
