//! Parse the embedded library into Base/Component values.
//!
//! The library is embedded at build time via `include_dir!`. `build.rs` has
//! already validated it, so parse failures here are programmer/library errors
//! (never user input) and panic.
use crate::model::{Base, Component};
use include_dir::{include_dir, Dir};

static LIBRARY: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/library");

pub struct Registry {
    bases: Vec<Base>,
    components: Vec<Component>,
}

pub fn load() -> Registry {
    let mut bases = Vec::new();
    if let Some(bases_root) = LIBRARY.get_dir("bases") {
        for base_dir in bases_root.dirs() {
            let toml_path = base_dir.path().join("base.toml");
            if let Some(f) = LIBRARY.get_file(&toml_path) {
                let text = f.contents_utf8().expect("utf8 base.toml");
                let base: Base = toml::from_str(text).expect("valid base.toml");
                bases.push(base);
            }
        }
    }

    let mut components = Vec::new();
    if let Some(comp_root) = LIBRARY.get_dir("components") {
        for lang_dir in comp_root.dirs() {
            for comp_dir in lang_dir.dirs() {
                let toml_path = comp_dir.path().join("component.toml");
                if let Some(f) = LIBRARY.get_file(&toml_path) {
                    let text = f.contents_utf8().expect("utf8 component.toml");
                    let c: Component = toml::from_str(text).expect("valid component.toml");
                    components.push(c);
                }
            }
        }
    }

    bases.sort_by(|a, b| a.name.cmp(&b.name));
    components.sort_by(|a, b| a.name.cmp(&b.name));
    Registry { bases, components }
}

impl Registry {
    pub fn base(&self, name: &str) -> Option<&Base> {
        self.bases.iter().find(|b| b.name == name)
    }

    pub fn component(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }

    /// Look up a component by name *and* applicable base. Component names are
    /// only unique within a language (e.g. both Rust and Go define `ci-github`),
    /// so generation must disambiguate by the base in play.
    pub fn component_for(&self, name: &str, base: &str) -> Option<&Component> {
        self.components
            .iter()
            .find(|c| c.name == name && c.bases.iter().any(|b| b == base))
    }

    pub fn bases(&self) -> Vec<&Base> {
        self.bases.iter().collect()
    }

    pub fn components_for(&self, base: &str) -> Vec<&Component> {
        self.components
            .iter()
            .filter(|c| c.bases.iter().any(|b| b == base))
            .collect()
    }

    /// `(relative_path, contents)` for a base's `files/` dir, with the `.j2`
    /// suffix stripped from output paths. Sorted by path (deterministic). The
    /// returned tuple is `(output_path, template_text)`.
    pub fn base_files(&self, base: &str) -> Vec<(String, String)> {
        let dir_path = format!("bases/{base}/files");
        let mut out = Vec::new();
        if let Some(dir) = LIBRARY.get_dir(&dir_path) {
            collect_files(dir, &dir_path, &mut out);
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Raw template text for a component file. `lang` ("rust"/"go") disambiguates
    /// components that share a name across languages (e.g. `ci-github`).
    pub fn component_template(&self, lang: &str, component: &str, rel: &str) -> Option<String> {
        let p = format!("components/{lang}/{component}/{rel}");
        LIBRARY
            .get_file(&p)
            .and_then(|f| f.contents_utf8().map(str::to_string))
    }
}

fn collect_files(dir: &Dir<'_>, root: &str, out: &mut Vec<(String, String)>) {
    for f in dir.files() {
        let full = f.path().to_string_lossy().to_string();
        let rel = full.strip_prefix(&format!("{root}/")).unwrap_or(&full);
        let rel = rel.strip_suffix(".j2").unwrap_or(rel).to_string();
        let contents = f.contents_utf8().unwrap_or_default().to_string();
        out.push((rel, contents));
    }
    for sub in dir.dirs() {
        collect_files(sub, root, out);
    }
}
