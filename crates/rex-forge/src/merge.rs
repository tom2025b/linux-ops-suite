//! Pure merge layer: anchored injection + dependency merge -> FileTree.
use crate::error::RenderError;
use crate::filetree::FileTree;
use crate::model::Selection;
use crate::registry::Registry;
use crate::render::{context, render_str};
use crate::resolve::ResolvePlan;

pub struct Generated {
    pub tree: FileTree,
    pub notes: Vec<String>,
}

/// Insert `fragment` immediately after the first line containing the anchor
/// comment, keeping the anchor line so multiple components can target it.
pub fn inject_at_anchor(body: &str, anchor: &str, fragment: &str) -> Result<String, RenderError> {
    let needle = format!("// {anchor}");
    let mut out = String::with_capacity(body.len() + fragment.len() + 1);
    let mut injected = false;
    for line in body.lines() {
        out.push_str(line);
        out.push('\n');
        if !injected && line.contains(&needle) {
            out.push_str(fragment.trim_end());
            out.push('\n');
            injected = true;
        }
    }
    if !injected {
        return Err(RenderError::MissingAnchor {
            target: String::new(),
            anchor: anchor.to_string(),
        });
    }
    Ok(out)
}

/// Merge `deps` into the base manifest's `[dependencies]` and re-serialize.
/// toml::Table serializes keys sorted, giving deterministic output.
pub fn merge_dependencies(base_manifest: &str, deps: &toml::Table) -> String {
    let mut doc: toml::Table = toml::from_str(base_manifest).unwrap_or_default();
    let table = doc
        .entry("dependencies".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let toml::Value::Table(existing) = table {
        for (k, v) in deps {
            existing.insert(k.clone(), v.clone());
        }
    }
    toml::to_string_pretty(&doc).unwrap_or_default()
}

/// Full generation pipeline: render base files, apply each component's files,
/// injections, dependencies and notes, in (already sorted) component order.
pub fn generate(
    reg: &Registry,
    plan: &ResolvePlan,
    sel: &Selection,
) -> Result<Generated, RenderError> {
    let ctx = context(sel);
    let mut tree = FileTree::new();
    let mut notes = Vec::new();

    let manifest_path = if sel.base.starts_with("rust") {
        "Cargo.toml"
    } else {
        "go.mod"
    };

    // 1. Render base files; hold the manifest aside for dep merging.
    let mut manifest_body = String::new();
    for (rel, raw) in reg.base_files(&plan.base) {
        let rendered = render_str(&raw, &ctx, &rel)?;
        if rel == manifest_path {
            manifest_body = rendered;
        } else {
            tree.insert(rel, rendered);
        }
    }

    // 2. Apply each component.
    let mut merged_deps = toml::Table::new();
    for name in &plan.components {
        let Some(comp) = reg.component(name) else { continue };

        for f in &comp.files {
            if let Some(raw) = reg.component_template(name, &f.template) {
                let rendered = render_str(&raw, &ctx, &f.path)?;
                tree.insert(f.path.clone(), rendered);
            }
        }

        for inj in &comp.injects {
            if let Some(raw) = reg.component_template(name, &inj.template) {
                let fragment = render_str(&raw, &ctx, &inj.template)?;
                // Injections target tree files (the manifest is handled separately).
                if let Some(current) = tree.get(&inj.target) {
                    let updated =
                        inject_at_anchor(current, &inj.anchor, &fragment).map_err(|e| match e {
                            RenderError::MissingAnchor { anchor, .. } => RenderError::MissingAnchor {
                                target: inj.target.clone(),
                                anchor,
                            },
                            other => other,
                        })?;
                    tree.insert(inj.target.clone(), updated);
                }
            }
        }

        for (k, v) in &comp.dependencies {
            merged_deps.insert(k.clone(), v.clone());
        }
        for note in &comp.notes {
            notes.push(note.text.clone());
        }
    }

    // 3. Merge deps into the manifest and insert it last.
    let final_manifest = merge_dependencies(&manifest_body, &merged_deps);
    tree.insert(manifest_path, final_manifest);

    Ok(Generated { tree, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_after_anchor_and_keeps_anchor() {
        let body = "// rex:imports\nfn main() {}\n";
        let out = inject_at_anchor(body, "rex:imports", "use clap::Parser;").unwrap();
        assert!(out.contains("// rex:imports"));
        assert!(out.contains("use clap::Parser;"));
        let out2 = inject_at_anchor(&out, "rex:imports", "mod cli;").unwrap();
        assert!(out2.contains("mod cli;") && out2.contains("use clap::Parser;"));
    }

    #[test]
    fn missing_anchor_errors() {
        let err = inject_at_anchor("fn main(){}", "rex:init", "x").unwrap_err();
        assert!(err.to_string().contains("rex:init"));
    }

    #[test]
    fn merge_dependencies_sorts_and_inserts() {
        let base = "[package]\nname = \"x\"\n\n[dependencies]\n";
        let mut deps = toml::Table::new();
        deps.insert("tracing".into(), toml::Value::String("0.1".into()));
        deps.insert("anyhow".into(), toml::Value::String("1".into()));
        let out = merge_dependencies(base, &deps);
        let a = out.find("anyhow").unwrap();
        let t = out.find("tracing").unwrap();
        assert!(a < t, "deps should be alphabetically sorted");
    }
}
