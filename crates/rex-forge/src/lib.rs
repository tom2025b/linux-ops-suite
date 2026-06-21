//! rex-forge — TUI-first project scaffolder for Rust and Go.
#![forbid(unsafe_code)]

pub mod cli;
pub mod error;
pub mod filetree;
pub mod merge;
pub mod model;
pub mod registry;
pub mod render;
pub mod resolve;
pub mod writer;

use crate::cli::NewArgs;
use crate::error::{ForgeError, WriteError};
use crate::model::Selection;
use crate::registry::Registry;
use crate::writer::{write, WriteOpts};
use std::path::Path;

/// Run the non-interactive `new` flow. (Interactive TUI is wired in a later task.)
pub fn run_new(reg: &Registry, args: &NewArgs) -> Result<(), ForgeError> {
    let name = args
        .name
        .clone()
        .ok_or_else(|| ForgeError::Write(WriteError::Io("project name required".into())))?;
    let base = match &args.base {
        Some(b) => b.clone(),
        None => {
            return Err(ForgeError::Write(WriteError::Io(
                "--base required (interactive mode lands later)".into(),
            )));
        }
    };
    let components: Vec<String> = args
        .with
        .as_deref()
        .map(|s| {
            s.split(',')
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let display_name = Path::new(&name)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.clone());

    let sel = Selection {
        base,
        components,
        project_name: display_name,
        license: args.license.clone().unwrap_or_else(|| "MIT".into()),
        author: args.author.clone().unwrap_or_default(),
    };

    let plan = resolve::resolve(reg, &sel.base, &sel.components)?;
    let generated = merge::generate(reg, &plan, &sel)?;

    let opts = WriteOpts { force: args.force, dry_run: args.dry_run, git: args.git };
    let dest = Path::new(&name);
    write(&generated.tree, dest, &opts)?;

    if args.dry_run {
        println!("{name}/");
        print!("{}", generated.tree.render_tree());
        println!("(dry run — nothing written)");
    } else {
        println!(
            "Created {} ({} files, base {})",
            name,
            generated.tree.paths().len(),
            sel.base
        );
        if !generated.notes.is_empty() {
            println!("  notes:");
            for n in &generated.notes {
                println!("    • {n}");
            }
        }
    }
    Ok(())
}

/// Print available bases and components as plain text.
pub fn run_list(reg: &Registry) {
    println!("bases:");
    for b in reg.bases() {
        println!("  {:10} {}", b.name, b.summary);
    }
    println!("components:");
    for b in reg.bases() {
        for c in reg.components_for(&b.name) {
            println!("  {:10} [{}] {}", c.name, b.name, c.summary);
        }
    }
}
