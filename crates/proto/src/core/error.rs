use std::path::PathBuf; // owned, heap-allocated filesystem path (vs &Path borrow)

// `thiserror::Error` derives the std Error trait + Display from our attributes.
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    // ---- Filesystem-level failures -----------------------------------------
    // The protocol directory or a specific file could not be read. We keep the
    // PATH that failed (so the message can name it) and `#[from]`-wrap the
    // underlying io::Error... but std::io::Error doesn't carry the path, so we
    // build this variant manually in the loader (see Io below for the auto case).
    #[error("could not read protocol file: {path}")]
    ReadFile {
        path: PathBuf, // which file we failed on — the user needs to know
        #[source] // marks the wrapped cause for the error-source chain
        source: std::io::Error,
    },

    // A directory we expected to scan (e.g. the protocols dir) was missing or
    // unreadable. Separate from ReadFile so the CLI can say "no protocols dir".
    #[error("could not read protocols directory: {path}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    // A file we tried to WRITE (a session record, the Workstate feed) could not
    // be written. Separate from ReadFile so the message says "could not write"
    // — the failure mode (permissions, full disk) and the fix differ from a read.
    #[error("could not write file: {path}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    // ---- Parsing failures --------------------------------------------------
    // The file was read but isn't valid YAML, or doesn't match the Protocol
    // shape (e.g. a step missing its `id`). We keep the path for context and
    // wrap serde_yaml's own error (with line/column) as the #[source] cause.
    //
    // The Display string deliberately does NOT interpolate {source}: callers
    // render the cause chain themselves (main.rs via anyhow's `{:#}`, the
    // `validate` command via `error_chain`), so embedding it here too would print
    // serde's detail TWICE — the duplication this used to produce.
    #[error("invalid protocol YAML in {path}")]
    ParseYaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    // A JSON file (a saved session, or a feed we read back) didn't parse. Kept
    // DISTINCT from ParseYaml (different format, different producer) and from
    // Validation (which is for rule violations, not malformed input). Before
    // this variant existed, a corrupt session was mislabelled a "validation"
    // failure, which read as if the data were merely out of policy — it isn't,
    // it's unreadable. Carrying the path lets the message name the bad file.
    // Like ParseYaml, the Display omits {source} to avoid double-printing.
    #[error("invalid JSON in {path}")]
    ParseJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    // We failed to TURN A VALUE INTO its serialized form (e.g. serializing a
    // Session to JSON before writing it). This essentially never happens for our
    // plain data types, but the previous code papered over it with `.expect()`,
    // which would PANIC the whole process on the one-in-a-million failure. A
    // typed variant lets us bubble it up and exit cleanly instead. `what` names
    // the thing we were serializing so the message is specific.
    #[error("could not serialize {what}: {source}")]
    Serialize {
        what: &'static str,
        #[source]
        source: serde_json::Error,
    },

    // ---- Semantic validation failures --------------------------------------
    // The file parsed into a Protocol, but the protocol is internally wrong:
    // duplicate step ids, an empty step list, a blank title, etc. These aren't
    // syntax errors — they're rule violations — so they get their own variant
    // carrying a human-readable explanation produced by the validator.
    #[error("protocol '{id}' failed validation: {reason}")]
    Validation {
        id: String,     // the protocol's declared id (or filename fallback)
        reason: String, // what rule it broke, in plain language
    },

    // ---- Lookup failures ---------------------------------------------------
    // The user asked to run/validate a protocol id that doesn't exist in the
    // protocols directory. Distinct from ReadFile: the dir is fine, the id isn't.
    #[error("no protocol found with id '{id}'")]
    NotFound { id: String },

    // ---- Catch-all for genuinely generic IO --------------------------------
    // `#[from]` auto-derives `From<std::io::Error>` for this variant, so a bare
    // `?` on an io operation that has no useful path context still compiles.
    // Prefer the path-carrying variants above when you know the path.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

// -----------------------------------------------------------------------------
// error_chain — render an error and its #[source] causes as "msg: cause: cause".
// -----------------------------------------------------------------------------
// Our Parse* variants omit {source} from their own Display (so the cause isn't
// double-printed when a caller already walks the chain). Code paths that print a
// ProtoError directly with `{}` — e.g. the per-file lines in `proto validate` —
// use this to include the cause once, matching what anyhow's `{:#}` shows for
// the same error elsewhere.
pub fn error_chain(error: &dyn std::error::Error) -> String {
    let mut rendered = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        rendered.push_str(": ");
        rendered.push_str(&cause.to_string());
        source = cause.source();
    }
    rendered
}
