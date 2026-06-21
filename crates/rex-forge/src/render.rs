//! Pure template rendering via minijinja.
use crate::error::RenderError;
use crate::model::Selection;
use minijinja::{context, Environment, Value};

/// Build the shared render context available to every template.
pub fn context(sel: &Selection) -> Value {
    let language = if sel.base.starts_with("rust") {
        "rust"
    } else {
        "go"
    };
    context! {
        project_name => sel.project_name.clone(),
        base => sel.base.clone(),
        language => language,
        license => sel.license.clone(),
        author => sel.author.clone(),
        components => sel.components.clone(),
    }
}

/// Render one template string. `label` identifies the file in errors.
pub fn render_str(template: &str, ctx: &Value, label: &str) -> Result<String, RenderError> {
    let mut env = Environment::new();
    env.add_template("t", template)
        .and_then(|()| env.get_template("t"))
        .and_then(|t| t.render(ctx))
        .map_err(|e| RenderError::Template {
            file: label.to_string(),
            reason: e.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Selection;

    fn sel() -> Selection {
        Selection {
            base: "rust-bin".into(),
            components: vec!["clap".into()],
            project_name: "myapp".into(),
            license: "MIT".into(),
            author: "tomb".into(),
        }
    }

    #[test]
    fn renders_project_name() {
        let ctx = context(&sel());
        let out = render_str("name = {{ project_name }}", &ctx, "test").unwrap();
        assert_eq!(out, "name = myapp");
    }

    #[test]
    fn components_membership_works_in_template() {
        let ctx = context(&sel());
        let out = render_str(
            "{% if \"clap\" in components %}yes{% else %}no{% endif %}",
            &ctx,
            "t",
        )
        .unwrap();
        assert_eq!(out, "yes");
    }

    #[test]
    fn bad_template_yields_render_error_with_label() {
        let ctx = context(&sel());
        let err = render_str("{{ unclosed ", &ctx, "main.rs").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("main.rs"));
    }
}
