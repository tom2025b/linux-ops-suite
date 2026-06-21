//! Validate the library at build time: every .toml must parse, and every
//! component's [[inject]] anchor must exist in the target base file. Fail the
//! build otherwise so a broken library can never ship.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn main() {
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("library");
    println!("cargo:rerun-if-changed={}", lib.display());

    let mut errors = Vec::new();

    // 1. Every .toml must parse.
    visit_toml(&lib, &mut errors);

    // 2. Collect base anchor sets, then check component injects reference a
    //    known anchor on a base they declare.
    let anchors = collect_base_anchors(&lib, &mut errors);
    check_injects(&lib, &anchors, &mut errors);

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("rex-forge library error: {e}");
        }
        panic!("invalid rex-forge library ({} error(s))", errors.len());
    }
}

fn visit_toml(dir: &Path, errors: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_toml(&path, errors);
        } else if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            match fs::read_to_string(&path) {
                Ok(s) => {
                    if let Err(e) = toml::from_str::<toml::Table>(&s) {
                        errors.push(format!("{}: {e}", path.display()));
                    }
                }
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
    }
}

/// Map base name -> set of anchor strings declared in its base.toml.
fn collect_base_anchors(lib: &Path, errors: &mut Vec<String>) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let bases_dir = lib.join("bases");
    let Ok(entries) = fs::read_dir(&bases_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let toml_path = entry.path().join("base.toml");
        let Ok(text) = fs::read_to_string(&toml_path) else {
            continue;
        };
        let Ok(table) = toml::from_str::<toml::Table>(&text) else {
            continue;
        };
        let name = table
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            errors.push(format!("{}: missing name", toml_path.display()));
            continue;
        }
        let anchors = table
            .get("anchors")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        out.insert(name, anchors);
    }
    out
}

/// Each component [[inject]] must target an anchor that exists in at least one
/// base the component declares.
fn check_injects(lib: &Path, anchors: &BTreeMap<String, Vec<String>>, errors: &mut Vec<String>) {
    let comp_root = lib.join("components");
    visit_components(&comp_root, anchors, errors);
}

fn visit_components(dir: &Path, anchors: &BTreeMap<String, Vec<String>>, errors: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_components(&path, anchors, errors);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("component.toml") {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(table) = toml::from_str::<toml::Table>(&text) else {
                continue;
            };
            let comp_name = table.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let bases: Vec<String> = table
                .get("bases")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let injects = table.get("inject").and_then(|v| v.as_array());
            if let Some(injects) = injects {
                for inj in injects {
                    let Some(anchor) = inj.get("anchor").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let ok = bases.iter().any(|b| {
                        anchors
                            .get(b)
                            .map(|set| set.iter().any(|a| a == anchor))
                            .unwrap_or(false)
                    });
                    if !ok {
                        errors.push(format!(
                            "component `{comp_name}`: inject anchor `{anchor}` not found in any declared base {bases:?}"
                        ));
                    }
                }
            }
        }
    }
}
