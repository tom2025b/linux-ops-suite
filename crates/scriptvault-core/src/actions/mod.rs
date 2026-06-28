// actions — frontend-agnostic, process-spawning actions (open in editor, run).
// In core because spawning is identical from any frontend; the caller handles UI
// concerns (the TUI suspends its alternate screen first). Clipboard ("copy path")
// stays in the binary — it differs between a terminal and a webview. No `unwrap`
// or `panic`: every failure returns an `Err` with context.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Read};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Result, ScriptVaultError};
use crate::model::{Language, ScriptEntry};

/// Default guardrail for every ScriptVault-launched script. A personal script can
/// be slow, but it must not hang ScriptVault forever.
pub const DEFAULT_RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Default amount of stdout/stderr retained by capture paths. Reader threads
/// continue draining after this bound so pipes cannot deadlock the child.
pub const DEFAULT_CAPTURE_BYTES: usize = 256 * 1024;

/// Polling interval for child status. Short enough to feel instant, long enough
/// to avoid spinning during normal runs.
const WAIT_POLL: Duration = Duration::from_millis(25);

/// Grace period between TERM and KILL when cancelling a process group.
const KILL_GRACE: Duration = Duration::from_millis(250);

/// How many times to retry a direct exec that fails with ETXTBSY, and how long
/// to wait between tries. A handful of short waits comfortably covers the
/// write-then-exec race without a perceptible delay to the user.
const EXEC_BUSY_RETRIES: u32 = 5;
const EXEC_BUSY_BACKOFF: Duration = Duration::from_millis(20);

/// Options shared by foreground run and captured run. `None` timeout means "wait
/// forever", but every public convenience path uses the bounded default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOptions {
    /// Maximum wall-clock runtime before ScriptVault kills the process group.
    pub timeout: Option<Duration>,
    /// Maximum bytes retained from each output stream in capture mode.
    pub max_output_bytes: usize,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            timeout: Some(DEFAULT_RUN_TIMEOUT),
            max_output_bytes: DEFAULT_CAPTURE_BYTES,
        }
    }
}

/// The final state of a child process. `exit_code == None` means the OS did not
/// report a normal code (signal/forced kill/unknown), not success.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WaitOutcome {
    /// The child's normal exit code, when available.
    pub exit_code: Option<i32>,
    /// True when ScriptVault killed the process for exceeding the timeout.
    pub timed_out: bool,
}

impl WaitOutcome {
    /// A clean run is explicitly exit code 0 and not a timeout.
    pub fn success(self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }
}

/// Captured stdout/stderr plus the real child outcome. Output is bounded per
/// stream, with truncation flags so callers can tell a snippet from the full log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl RunOutput {
    /// A clean capture is explicitly exit code 0 and not a timeout.
    pub fn success(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }
}

/// A child process owned by ScriptVault. Dropping it kills and reaps the whole
/// process group, which is the critical leak/hang guard for TUI live runs.
pub struct ManagedChild {
    child: Child,
    finished: bool,
}

impl ManagedChild {
    /// Wrap a newly spawned child. Every script child is spawned into a fresh
    /// process group on Unix before reaching this type.
    fn new(child: Child) -> Self {
        Self {
            child,
            finished: false,
        }
    }

    /// The OS pid of the direct child.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Take the stdout pipe for a reader thread. Returns `None` after it has
    /// already been taken or when the child was not spawned with piped stdout.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    /// Take the stderr pipe for a reader thread.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    /// Non-blocking status probe. Marks the child finished when a status arrives.
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match self.child.try_wait()? {
            Some(status) => {
                self.finished = true;
                Ok(Some(status))
            }
            None => Ok(None),
        }
    }

    /// Wait for completion with an optional timeout. On timeout, terminates and
    /// reaps the process group before returning.
    pub fn wait_for(&mut self, timeout: Option<Duration>) -> Result<WaitOutcome> {
        let started = Instant::now();

        loop {
            if let Some(status) = self.try_wait().map_err(|source| {
                ScriptVaultError::action_io("failed while waiting for script", source)
            })? {
                return Ok(WaitOutcome {
                    exit_code: status.code(),
                    timed_out: false,
                });
            }

            if timeout.is_some_and(|limit| started.elapsed() >= limit) {
                let exit_code = self.kill_and_reap();
                return Ok(WaitOutcome {
                    exit_code,
                    timed_out: true,
                });
            }

            thread::sleep(WAIT_POLL);
        }
    }

    /// Kill the process group and reap the direct child. Best-effort by design:
    /// this is cleanup, and callers still need progress even if the child already
    /// exited or the OS refuses a signal.
    pub fn kill_and_reap(&mut self) -> Option<i32> {
        if self.finished {
            return None;
        }

        terminate_process_group(self.id());
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
        thread::sleep(KILL_GRACE);

        if let Ok(Some(status)) = self.child.try_wait() {
            self.finished = true;
            return status.code();
        }

        kill_process_group(self.id());
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
        let status = self.child.wait().ok();
        self.finished = true;
        status.and_then(|s| s.code())
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.kill_and_reap();
    }
}

/// Open a script in the resolved `editor` (which may carry args, e.g. "code
/// --wait"): first token is the program, the rest are leading args, path last.
/// Stdio is inherited; the caller suspends any alternate screen first.
pub fn open_in_editor(entry: &ScriptEntry, editor: &str) -> Result<()> {
    open_in_editor_at_line(entry, editor, None)
}

/// Like [`open_in_editor`], optionally jumping to a line. For `Some(n)`, inserts
/// the right go-to-line arg for the detected editor family (`+N file` for
/// vim/nano/emacs/…, `--goto file:N` for vscode), or nothing for the rest —
/// never an error, a missing jump must not block opening.
pub fn open_in_editor_at_line(
    entry: &ScriptEntry,
    editor: &str,
    line: Option<usize>,
) -> Result<()> {
    // Split the editor command into program + leading args. An empty/whitespace
    // editor string is a configuration error worth reporting clearly.
    let mut parts = editor.split_whitespace();
    let program = parts.next().ok_or_else(|| {
        ScriptVaultError::action("no editor configured (set `editor:` in config or $EDITOR)")
    })?;
    let leading_args: Vec<&str> = parts.collect();

    let mut cmd = Command::new(program);
    cmd.args(&leading_args);

    // Add a "jump to line" argument when requested and the editor supports one.
    // `goto_line_args` returns the extra argv tokens (possibly empty).
    if let Some(n) = line {
        for arg in goto_line_args(program, &entry.path, n) {
            cmd.arg(arg);
        }
    } else {
        cmd.arg(&entry.path);
    }

    let status = cmd
        // Inherit stdin/stdout/stderr so the editor is interactive.
        .status()
        .map_err(|e| {
            ScriptVaultError::action_io(format!("failed to launch editor `{program}`"), e)
        })?;

    // A non-zero editor exit is reported but not treated as catastrophic.
    if !status.success() {
        return Err(ScriptVaultError::action(format!(
            "editor `{program}` exited with status {status}"
        )));
    }
    Ok(())
}

/// Build the argv tokens that open `path` at `line` for the given editor program.
/// Returns the FULL set of tokens to append (so the path placement differs by
/// family). Unknown editors get just the path (open at top, no jump).
fn goto_line_args(program: &str, path: &std::path::Path, line: usize) -> Vec<OsString> {
    // Detect by basename so "/usr/bin/nvim" and "nvim" behave the same.
    let base = program.rsplit('/').next().unwrap_or(program);
    let path_os = path.as_os_str().to_owned();
    match base {
        // VS Code family: `--goto file:line`.
        "code" | "codium" | "code-insiders" => {
            let mut goto = path.as_os_str().to_owned();
            goto.push(format!(":{line}"));
            vec![OsString::from("--goto"), goto]
        }
        // The `+N file` convention, supported by the common terminal editors.
        "vim" | "nvim" | "vi" | "nano" | "emacs" | "emacsclient" | "kak" | "joe" | "micro" => {
            vec![OsString::from(format!("+{line}")), path_os]
        }
        // Unknown editor: just open the file (no jump). Not an error.
        _ => vec![path_os],
    }
}

/// Run a script as a child process, inheriting stdio so its output is visible.
/// This convenience path uses [`DEFAULT_RUN_TIMEOUT`].
pub fn run(entry: &ScriptEntry) -> Result<()> {
    run_with_options(entry, RunOptions::default())
}

/// Run a script with explicit process-control options.
pub fn run_with_options(entry: &ScriptEntry, options: RunOptions) -> Result<()> {
    let label = entry_label(entry);
    let mut child = spawn_script(&entry.path, Some(entry.lang), ChildIo::Inherit)?;
    let outcome = child.wait_for(options.timeout)?;
    finish_outcome(outcome, &label, options.timeout)
}

/// Run a script non-interactively and capture bounded stdout/stderr. This is the
/// safe replacement for `Command::output()`: it enforces timeout, avoids unbounded
/// memory growth, and returns the real exit code.
pub fn capture(entry: &ScriptEntry) -> Result<RunOutput> {
    capture_with_options(entry, RunOptions::default())
}

/// Capture with explicit process-control options.
pub fn capture_with_options(entry: &ScriptEntry, options: RunOptions) -> Result<RunOutput> {
    let mut child = spawn_script(&entry.path, Some(entry.lang), ChildIo::Capture)?;
    let stdout = child
        .take_stdout()
        .map(|pipe| spawn_limited_reader(pipe, options.max_output_bytes));
    let stderr = child
        .take_stderr()
        .map(|pipe| spawn_limited_reader(pipe, options.max_output_bytes));

    let outcome = child.wait_for(options.timeout)?;
    let (stdout, stdout_truncated) = join_reader(stdout);
    let (stderr, stderr_truncated) = join_reader(stderr);

    Ok(RunOutput {
        exit_code: outcome.exit_code,
        timed_out: outcome.timed_out,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

/// Spawn a live-capture child for an adapter that wants to stream the pipes
/// itself. The returned child still owns process-group cleanup on drop.
pub fn spawn_live_capture(path: &Path) -> Result<ManagedChild> {
    spawn_script(path, None, ChildIo::Capture)
}

/// How the child's standard handles are wired.
#[derive(Debug, Clone, Copy)]
enum ChildIo {
    Inherit,
    Capture,
}

/// A concrete argv plan for launching a script.
#[derive(Debug)]
struct CommandSpec {
    program: OsString,
    args: Vec<OsString>,
}

impl CommandSpec {
    /// Direct exec: the script path is the program and the OS handles shebangs.
    fn direct(path: &Path) -> Self {
        Self {
            program: path.as_os_str().to_owned(),
            args: Vec::new(),
        }
    }

    /// Interpreter exec: program + interpreter args + script path.
    fn interpreted(program: OsString, mut args: Vec<OsString>, path: &Path) -> Self {
        args.push(path.as_os_str().to_owned());
        Self { program, args }
    }
}

/// Spawn a script with the ScriptVault policy:
///   1. direct exec first, retrying ETXTBSY;
///   2. if not executable, honor its shebang interpreter;
///   3. if it is a shell script with no shebang, run `sh <path>`;
///   4. otherwise fail clearly instead of feeding Python/Ruby/etc. to `sh`.
fn spawn_script(path: &Path, lang_hint: Option<Language>, io: ChildIo) -> Result<ManagedChild> {
    let direct = CommandSpec::direct(path);
    match spawn_with_retry(&direct, io) {
        Ok(child) => Ok(child),
        Err(err) if is_not_executable(&err) => {
            if let Some(fallback) = fallback_command(path, lang_hint).map_err(|source| {
                ScriptVaultError::action_io(
                    format!("failed to inspect shebang for {}", path.display()),
                    source,
                )
            })? {
                return spawn_command(&fallback, io).map_err(|source| {
                    ScriptVaultError::action_io(
                        format!("failed to run {} via interpreter", path.display()),
                        source,
                    )
                });
            }

            Err(ScriptVaultError::action(format!(
                "{} is not executable and has no runnable shebang or shell fallback",
                path.display()
            )))
        }
        Err(err) => Err(ScriptVaultError::action_io(
            format!("failed to run {}", path.display()),
            err,
        )),
    }
}

/// Direct exec can fail briefly with ETXTBSY after an edit/chmod race. Retry only
/// that one transient error; all other spawn errors are policy decisions.
fn spawn_with_retry(spec: &CommandSpec, io: ChildIo) -> std::io::Result<ManagedChild> {
    let mut attempt = 0;
    loop {
        match spawn_command(spec, io) {
            Ok(child) => return Ok(child),
            Err(err) if is_text_file_busy(&err) && attempt < EXEC_BUSY_RETRIES => {
                attempt += 1;
                thread::sleep(EXEC_BUSY_BACKOFF);
            }
            Err(err) => return Err(err),
        }
    }
}

/// Build and spawn one command spec, placing script processes in their own
/// process group on Unix so timeout/drop can kill descendants too.
fn spawn_command(spec: &CommandSpec, io: ChildIo) -> std::io::Result<ManagedChild> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args);

    match io {
        ChildIo::Inherit => {
            cmd.stdin(Stdio::inherit());
            cmd.stdout(Stdio::inherit());
            cmd.stderr(Stdio::inherit());
        }
        ChildIo::Capture => {
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    cmd.spawn().map(ManagedChild::new)
}

/// Find a safe fallback command for a script that the OS would not execute
/// directly. Shebang interpreters win; `sh` is only for shell scripts.
fn fallback_command(
    path: &Path,
    lang_hint: Option<Language>,
) -> std::io::Result<Option<CommandSpec>> {
    if let Some(spec) = shebang_command(path)? {
        return Ok(Some(spec));
    }

    if is_shell_script(path, lang_hint) {
        return Ok(Some(CommandSpec::interpreted(
            OsString::from("sh"),
            Vec::new(),
            path,
        )));
    }

    Ok(None)
}

/// Parse the first-line shebang into an interpreter command, including common
/// `/usr/bin/env python3` and `/usr/bin/env -S python3 -u` forms.
fn shebang_command(path: &Path) -> std::io::Result<Option<CommandSpec>> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let mut first = String::new();
    reader.read_line(&mut first)?;

    let Some(body) = first.trim_start().strip_prefix("#!") else {
        return Ok(None);
    };
    let mut tokens: Vec<&str> = body.split_whitespace().collect();
    if tokens.is_empty() {
        return Ok(None);
    }

    let program = tokens.remove(0);
    if basename(program) == "env" {
        return Ok(env_shebang_command(tokens, path));
    }

    Ok(Some(CommandSpec::interpreted(
        OsString::from(program),
        tokens.into_iter().map(OsString::from).collect(),
        path,
    )))
}

/// Interpret the useful subset of `env` shebangs without trying to emulate every
/// `env` option. The goal is to locate the interpreter token safely.
fn env_shebang_command(tokens: Vec<&str>, path: &Path) -> Option<CommandSpec> {
    let mut iter = tokens.into_iter();
    while let Some(token) = iter.next() {
        if token == "-S" {
            let mut split_tokens: Vec<&str> = Vec::new();
            for rest in iter {
                split_tokens.extend(rest.split_whitespace());
            }
            return interpreter_from_tokens(split_tokens, path);
        }

        if token.starts_with('-') || token.contains('=') {
            continue;
        }

        let args = iter.map(OsString::from).collect();
        return Some(CommandSpec::interpreted(OsString::from(token), args, path));
    }

    None
}

/// Convert `["python3", "-u"]` into `python3 -u <script>`.
fn interpreter_from_tokens(tokens: Vec<&str>, path: &Path) -> Option<CommandSpec> {
    let mut iter = tokens.into_iter();
    let program = iter.next()?;
    let args = iter.map(OsString::from).collect();
    Some(CommandSpec::interpreted(
        OsString::from(program),
        args,
        path,
    ))
}

/// Basename for a Unix-style shebang program path.
fn basename(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

/// True when a no-shebang script is safe to pass to `sh`. Non-executable Python
/// or Ruby files are deliberately rejected instead of misrun through the shell.
fn is_shell_script(path: &Path, lang_hint: Option<Language>) -> bool {
    if matches!(lang_hint, Some(Language::Bash)) {
        return true;
    }

    path.extension()
        .and_then(OsStr::to_str)
        .map(|ext| matches!(ext, "sh" | "bash" | "zsh"))
        .unwrap_or(false)
}

/// Spawn a reader thread that drains a pipe completely while retaining only a
/// bounded prefix in memory.
fn spawn_limited_reader<R>(reader: R, limit: usize) -> thread::JoinHandle<(Vec<u8>, bool)>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || read_limited(reader, limit))
}

/// Read all bytes from a stream, retaining at most `limit` bytes.
fn read_limited<R: Read>(mut reader: R, limit: usize) -> (Vec<u8>, bool) {
    let mut retained = Vec::with_capacity(limit.min(8192));
    let mut buf = [0_u8; 8192];
    let mut truncated = false;

    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let remaining = limit.saturating_sub(retained.len());
                if remaining > 0 {
                    retained.extend_from_slice(&buf[..n.min(remaining)]);
                }
                if n > remaining {
                    truncated = true;
                }
            }
            Err(_) => {
                truncated = true;
                break;
            }
        }
    }

    (retained, truncated)
}

/// Join a reader thread without letting a thread panic cross the core boundary.
fn join_reader(handle: Option<thread::JoinHandle<(Vec<u8>, bool)>>) -> (Vec<u8>, bool) {
    match handle {
        Some(handle) => handle.join().unwrap_or_else(|_| (Vec::new(), true)),
        None => (Vec::new(), false),
    }
}

/// Turn a finished child's outcome into a `Result`.
fn finish_outcome(outcome: WaitOutcome, label: &str, timeout: Option<Duration>) -> Result<()> {
    if outcome.success() {
        return Ok(());
    }

    if outcome.timed_out {
        let timeout = timeout
            .map(format_duration)
            .unwrap_or_else(|| "configured timeout".to_string());
        return Err(ScriptVaultError::action(format!(
            "{label} timed out after {timeout}"
        )));
    }

    match outcome.exit_code {
        Some(code) => Err(ScriptVaultError::action(format!(
            "{label} exited with status {code}"
        ))),
        None => Err(ScriptVaultError::action(format!(
            "{label} exited without a reported status"
        ))),
    }
}

/// Small human duration formatter for statuses and errors.
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 60 && secs.is_multiple_of(60) {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

/// True if a spawn error means "this file isn't directly executable" — the
/// signal to inspect a shebang or a shell fallback.
fn is_not_executable(err: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(
        err.kind(),
        ErrorKind::PermissionDenied | ErrorKind::InvalidInput
    ) || err.raw_os_error() == Some(8)
}

/// True if a spawn error is ETXTBSY ("text file busy") — a TRANSIENT condition
/// when exec'ing a file whose write handle the kernel hasn't released yet.
fn is_text_file_busy(err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::ExecutableFileBusy {
        return true;
    }
    err.raw_os_error() == Some(26)
}

/// A short human label for a script in error messages.
fn entry_label(entry: &ScriptEntry) -> String {
    entry.path.display().to_string()
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "kill"]
    fn c_kill(pid: i32, sig: i32) -> i32;
}

/// Ask the process group to terminate politely.
fn terminate_process_group(pid: u32) {
    signal_process_group(pid, Signal::Terminate);
}

/// Force-kill the process group.
fn kill_process_group(pid: u32) {
    signal_process_group(pid, Signal::Kill);
}

/// The two signals we need without pulling a libc dependency into core.
#[derive(Debug, Clone, Copy)]
enum Signal {
    Terminate,
    Kill,
}

impl Signal {
    #[cfg(unix)]
    fn number(self) -> i32 {
        match self {
            Signal::Terminate => 15,
            Signal::Kill => 9,
        }
    }
}

/// Send a signal to the child's process group on Unix, or just kill the direct
/// child on non-Unix. All script children are put in their own group by
/// `spawn_command`, so `-pid` addresses the whole job tree.
fn signal_process_group(pid: u32, signal: Signal) {
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else {
            return;
        };
        // SAFETY: `c_kill` is the POSIX `kill(2)` syscall. Passing a negative pid
        // targets the process group whose id is `pid`; the signal number is one
        // of the fixed constants above. We ignore the return code because cleanup
        // is best-effort and races with normal child exit.
        unsafe {
            let _ = c_kill(-pid, signal.number());
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (pid, signal);
    }
}

/// Delete a script file from disk.
///
/// Frontend-agnostic like `run`/`open_in_editor`, so a TUI or future GUI deletes
/// identically. Returns `Err` (never panics) if the file can't be removed — e.g.
/// it's already gone or permission is denied — for the caller to surface. We
/// remove ONLY the script file; the caller rebuilds its index afterward.
pub fn delete(entry: &ScriptEntry) -> Result<()> {
    std::fs::remove_file(&entry.path).map_err(|e| {
        ScriptVaultError::action_io(format!("failed to delete {}", entry.path.display()), e)
    })
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Language, MetaSource, ScriptMetadata};
    use std::fs;
    use std::path::PathBuf;

    fn tmp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("scriptvault-actions-{tag}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry_at(path: PathBuf) -> ScriptEntry {
        entry_at_lang(path, Language::Bash)
    }

    fn entry_at_lang(path: PathBuf, lang: Language) -> ScriptEntry {
        ScriptEntry {
            filename: path.file_name().unwrap().to_string_lossy().into_owned(),
            path,
            lang,
            meta: ScriptMetadata::default(),
            source: MetaSource::None,
        }
    }

    #[test]
    fn open_in_editor_with_empty_command_errors() {
        let entry = entry_at(PathBuf::from("/tmp/whatever.sh"));
        let err = open_in_editor(&entry, "   ").unwrap_err();
        assert!(err.to_string().contains("no editor configured"));
    }

    #[test]
    fn open_in_editor_uses_program_and_args() {
        // Use `true` as a stand-in "editor": it ignores its args and exits 0.
        // This verifies we can launch a program + pass the path without error.
        let entry = entry_at(PathBuf::from("/tmp/whatever.sh"));
        // `true` exists on any POSIX system; it returns success regardless.
        let result = open_in_editor(&entry, "true");
        assert!(result.is_ok());
    }

    #[test]
    fn open_at_line_passes_through_for_unknown_editor() {
        // `true` is not a known editor → no jump arg, but it must still launch
        // and succeed (the at-line variant never errors on a missing feature).
        let entry = entry_at(PathBuf::from("/tmp/whatever.sh"));
        assert!(open_in_editor_at_line(&entry, "true", Some(42)).is_ok());
    }

    #[test]
    fn goto_line_args_use_the_right_convention_per_editor() {
        let p = Path::new("/x/deploy.sh");
        // vim family → `+N`, then the path.
        assert_eq!(
            goto_line_args("nvim", p, 12),
            vec![OsString::from("+12"), OsString::from("/x/deploy.sh")]
        );
        // A full path to vim resolves by basename.
        assert_eq!(
            goto_line_args("/usr/bin/vim", p, 3)[0],
            OsString::from("+3")
        );
        // VS Code → `--goto file:N`.
        assert_eq!(
            goto_line_args("code", p, 7),
            vec![OsString::from("--goto"), OsString::from("/x/deploy.sh:7")]
        );
        // Unknown editor → just the path (open at top, no jump, no error).
        assert_eq!(
            goto_line_args("ed", p, 9),
            vec![OsString::from("/x/deploy.sh")]
        );
    }

    #[test]
    fn run_executable_script_succeeds() {
        let dir = tmp_dir("run-ok");
        let script = dir.join("ok.sh");
        fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        make_executable(&script);

        let entry = entry_at(script);
        assert!(run(&entry).is_ok());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_nonexecutable_shell_script_falls_back_to_shell() {
        // No +x bit and no shebang, but the extension/language says shell, so
        // `sh <path>` is the intentional fallback.
        let dir = tmp_dir("run-noexec-shell");
        let script = dir.join("plain.sh");
        fs::write(&script, "exit 0\n").unwrap();
        let entry = entry_at(script);
        assert!(run(&entry).is_ok());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_nonexecutable_shebang_uses_interpreter_not_shell() {
        // This is invalid shell but valid Python. If we accidentally route it
        // through `sh`, the test fails with a syntax error.
        let dir = tmp_dir("run-shebang-python");
        let script = dir.join("tool.py");
        fs::write(&script, "#!/usr/bin/env python3\nprint('ok')\n").unwrap();
        let entry = entry_at_lang(script, Language::Python);
        assert!(run(&entry).is_ok());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_nonexecutable_non_shell_without_shebang_does_not_use_sh() {
        let dir = tmp_dir("run-noexec-python-noshebang");
        let script = dir.join("tool.py");
        fs::write(&script, "print('ok')\n").unwrap();
        let entry = entry_at_lang(script, Language::Python);
        let err = run(&entry).unwrap_err();
        assert!(
            err.to_string().contains("no runnable shebang"),
            "got: {err:#}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_failing_script_reports_nonzero_exit() {
        let dir = tmp_dir("run-fail");
        let script = dir.join("fail.sh");
        fs::write(&script, "#!/bin/sh\nexit 3\n").unwrap();
        make_executable(&script);

        let entry = entry_at(script);
        let err = run(&entry).unwrap_err();
        assert!(
            err.to_string().contains("exited with status"),
            "got: {err:#}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_timeout_kills_long_script() {
        let dir = tmp_dir("run-timeout");
        let script = dir.join("slow.sh");
        fs::write(&script, "#!/bin/sh\nsleep 5\n").unwrap();
        make_executable(&script);

        let entry = entry_at(script);
        let err = run_with_options(
            &entry,
            RunOptions {
                timeout: Some(Duration::from_millis(75)),
                max_output_bytes: DEFAULT_CAPTURE_BYTES,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("timed out"), "got: {err:#}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn capture_records_nonzero_exit_and_output() {
        let dir = tmp_dir("capture-fail");
        let script = dir.join("fail.sh");
        fs::write(
            &script,
            "#!/bin/sh\necho stdout-line\necho stderr-line >&2\nexit 7\n",
        )
        .unwrap();
        make_executable(&script);

        let entry = entry_at(script);
        let output = capture(&entry).unwrap();
        assert_eq!(output.exit_code, Some(7));
        assert!(!output.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("stdout-line"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("stderr-line"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn capture_timeout_returns_timed_out_without_hanging() {
        let dir = tmp_dir("capture-timeout");
        let script = dir.join("slow.sh");
        fs::write(&script, "#!/bin/sh\necho before\nsleep 5\n").unwrap();
        make_executable(&script);

        let entry = entry_at(script);
        let output = capture_with_options(
            &entry,
            RunOptions {
                timeout: Some(Duration::from_millis(75)),
                max_output_bytes: 1024,
            },
        )
        .unwrap();
        assert!(output.timed_out);
        assert!(!output.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("before"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn capture_bounds_retained_output() {
        let dir = tmp_dir("capture-bound");
        let script = dir.join("spam.sh");
        fs::write(&script, "#!/bin/sh\nprintf 'abcdef'\n").unwrap();
        make_executable(&script);

        let entry = entry_at(script);
        let output = capture_with_options(
            &entry,
            RunOptions {
                timeout: Some(Duration::from_secs(1)),
                max_output_bytes: 3,
            },
        )
        .unwrap();
        assert_eq!(output.stdout, b"abc");
        assert!(output.stdout_truncated);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_removes_the_file_and_leaves_siblings() {
        let dir = tmp_dir("delete-ok");
        let target = dir.join("doomed.sh");
        let sibling = dir.join("keep.sh");
        fs::write(&target, "#!/bin/sh\necho bye\n").unwrap();
        fs::write(&sibling, "#!/bin/sh\necho hi\n").unwrap();

        let entry = entry_at(target.clone());
        assert!(delete(&entry).is_ok());
        assert!(!target.exists(), "deleted file must be gone");
        assert!(sibling.exists(), "sibling must be untouched");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_missing_file_reports_error_not_panic() {
        let dir = tmp_dir("delete-missing");
        let entry = entry_at(dir.join("not-here.sh"));
        let err = delete(&entry).unwrap_err();
        assert!(err.to_string().contains("failed to delete"), "got: {err:#}");
        fs::remove_dir_all(&dir).ok();
    }

    /// Set the executable bit on Unix; no-op elsewhere (tests still compile).
    fn make_executable(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).unwrap();
        }
        #[cfg(not(unix))]
        let _ = path;
    }
}
