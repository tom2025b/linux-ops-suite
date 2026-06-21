//! Pure TUI state machine. Drives the event loop; testable without a terminal.
use crate::model::{self, Selection};
use crate::registry::Registry;
use crate::resolve::resolve;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Base,
    Details,
    Components,
    Confirm,
    Done,
}

pub struct AppState {
    pub step: Step,
    pub project_name: String,
    pub license: String,
    pub author: String,
    pub git: bool,
    pub base: Option<String>,
    pub cursor: usize,
    pub filter: String,
    pub selected: Vec<String>,
    pub status: String,
}

impl AppState {
    pub fn new(_reg: &Registry, project_name: String) -> Self {
        Self {
            step: Step::Base,
            project_name,
            license: "MIT".into(),
            author: String::new(),
            git: false,
            base: None,
            cursor: 0,
            filter: String::new(),
            selected: Vec::new(),
            status: String::new(),
        }
    }

    pub fn visible_bases<'a>(&self, reg: &'a Registry) -> Vec<&'a model::Base> {
        reg.bases()
    }

    pub fn visible_components<'a>(&self, reg: &'a Registry) -> Vec<&'a model::Component> {
        let base = self.base.as_deref().unwrap_or("");
        reg.components_for(base)
            .into_iter()
            .filter(|c| self.filter.is_empty() || c.name.contains(&self.filter))
            .collect()
    }

    pub fn choose_base(&mut self, base: String) {
        self.base = Some(base);
        self.cursor = 0;
    }

    pub fn is_selected(&self, name: &str) -> bool {
        self.selected.iter().any(|s| s == name)
    }

    /// Toggle a component by name. Adding validates via the resolver; on a
    /// conflict the component is NOT added and `status` carries the reason.
    pub fn select_component_by_name(&mut self, name: &str) {
        self.status.clear();
        if self.is_selected(name) {
            self.selected.retain(|s| s != name);
            return;
        }
        let mut candidate = self.selected.clone();
        candidate.push(name.to_string());
        let base = self.base.clone().unwrap_or_default();
        match try_resolve(&base, &candidate) {
            Ok(final_set) => self.selected = final_set,
            Err(msg) => self.status = msg,
        }
    }

    pub fn move_down(&mut self, len: usize) {
        if len > 0 {
            self.cursor = (self.cursor + 1).min(len - 1);
        }
    }

    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn selection(&self) -> Selection {
        let mut components = self.selected.clone();
        components.sort();
        Selection {
            base: self.base.clone().unwrap_or_default(),
            components,
            project_name: self.project_name.clone(),
            license: self.license.clone(),
            author: self.author.clone(),
        }
    }
}

fn try_resolve(base: &str, components: &[String]) -> Result<Vec<String>, String> {
    let reg = crate::registry::load();
    match resolve(&reg, base, components) {
        Ok(plan) => Ok(plan.components),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    #[test]
    fn starts_on_base_step_with_bases_listed() {
        let reg = registry::load();
        let st = AppState::new(&reg, "myapp".into());
        assert_eq!(st.step, Step::Base);
        assert!(!st.visible_bases(&reg).is_empty());
    }

    #[test]
    fn toggling_a_valid_component_selects_it() {
        let reg = registry::load();
        let mut st = AppState::new(&reg, "myapp".into());
        st.choose_base("rust-bin".into());
        st.step = Step::Components;
        st.select_component_by_name("clap");
        assert!(st.is_selected("clap"));
        let sel = st.selection();
        assert!(sel.components.contains(&"clap".to_string()));
    }

    #[test]
    fn toggling_twice_deselects() {
        let reg = registry::load();
        let mut st = AppState::new(&reg, "myapp".into());
        st.choose_base("rust-bin".into());
        st.step = Step::Components;
        st.select_component_by_name("clap");
        st.select_component_by_name("clap");
        assert!(!st.is_selected("clap"));
    }

    #[test]
    fn valid_component_leaves_status_clear() {
        let reg = registry::load();
        let mut st = AppState::new(&reg, "myapp".into());
        st.choose_base("rust-bin".into());
        st.step = Step::Components;
        st.select_component_by_name("anyhow");
        assert!(st.is_selected("anyhow"));
        assert!(st.status.is_empty());
    }
}
