# rex-forge library schema

## Render context (available in every .j2 template)
- `project_name` — string, validated identifier
- `base` — base name, e.g. "rust-bin"
- `language` — "rust" | "go"
- `license` — e.g. "MIT"
- `author` — string (may be empty)
- `components` — list of selected component names (use `"clap" in components`)

## Base anchors
Base shared files declare anchors as line comments. Components target them via `[[inject]]`.
- Rust `src/main.rs.j2`: `// rex:imports`, `// rex:init`, `// rex:main`
- Rust `src/lib.rs.j2`: `// rex:imports`, `// rex:body`
- Go `main.go.j2`: `// rex:imports`, `// rex:init`, `// rex:main`
- Go `go.mod.j2`: `// rex:require`

An `[[inject]]` referencing an anchor absent from the target file is a build error.
