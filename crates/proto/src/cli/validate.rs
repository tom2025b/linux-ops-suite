use std::path::Path;

use anyhow::bail; // early-return with a formatted anyhow error

use crate::core::error::error_chain; // render a ProtoError + its cause chain
use crate::core::loader; // discover/load/validate

// `id` is Option<&str>: Some(name) => single-protocol mode; None => validate all.
pub fn handle(dir: &Path, id: Option<&str>) -> anyhow::Result<()> {
    match id {
        // ---- Single-protocol mode -----------------------------------------
        Some(id) => {
            // `find_one` locates + validates ONLY the requested protocol, so a
            // broken UNRELATED file in the same directory can't make this fail —
            // validating the file you asked about is the entire job. A successful
            // return means THIS protocol is valid; a failure names THIS protocol.
            let protocol = loader::find_one(dir, id)?;
            println!(
                "OK  {}  —  {}  ({} steps)",
                protocol.id,
                protocol.title,
                protocol.step_count()
            );
            Ok(())
        }

        // ---- Validate-all mode --------------------------------------------
        None => validate_all(dir),
    }
}

// Validate every file individually so ONE bad protocol doesn't hide the status
// of the others. We discover the raw paths, then load+validate each, collecting
// a pass/fail line per file. Returns an error at the end if any failed, so the
// process exit code reflects overall success — useful in scripts/CI.
fn validate_all(dir: &Path) -> anyhow::Result<()> {
    let paths = loader::discover(dir)?;

    if paths.is_empty() {
        println!("No protocols found in {}", dir.display());
        return Ok(());
    }

    // Track how many failed so we can set the exit status and print a summary.
    let mut failures = 0usize;

    for path in &paths {
        // Validate this one file. We do load + validate inline (rather than
        // calling find) so a parse error on file A still lets us report B and C.
        let result = loader::load_file(path).and_then(|p| {
            loader::check(path, &p)?; // content rules + id-matches-filename
            Ok(p)
        });

        match result {
            Ok(p) => println!("OK    {}  ({})", p.id, path.display()),
            // Print the precise reason inline (with its cause chain, so a parse
            // error still shows serde's line/column); keep going to check the rest.
            Err(e) => {
                println!("FAIL  {}  →  {}", path.display(), error_chain(&e));
                failures += 1;
            }
        }
    }

    // Non-zero exit when anything failed, so `proto validate && ...` works.
    if failures > 0 {
        bail!(
            "{} of {} protocol(s) failed validation",
            failures,
            paths.len()
        );
    }

    println!("\nAll {} protocol(s) valid.", paths.len());
    Ok(())
}
