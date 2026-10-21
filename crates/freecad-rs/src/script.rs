//! `freecad-rs script FILE.rhai` — embedded scripting.
//!
//! Runs a rhai script against a fresh sandboxed engine (`fc-script`) and
//! prints the honest transcript: what was added, set, bound, recomputed,
//! saved. Failure semantics mirror the CLI's:
//!
//! - a script whose recompute fails has the error recorded in the run;
//!   the transcript still prints (what happened is on record) and the
//!   process exits 1 with the same message style as the CLI's recompute
//!   failure output;
//! - a rhai parse/runtime error is a clear `error: …` message on stderr
//!   and exit 1 (the transcript printed so far stays on stdout);
//! - like every command, exit 0 only when nothing failed.

/// Run `path` as a rhai script. The transcript goes to stdout, warnings
/// (from `open_document` loads) and errors to stderr.
pub fn run(path: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read '{path}': {e}"))?;
    let mut engine = fc_script::ScriptEngine::new();
    let result = engine.run(&text);
    for event in engine.events() {
        println!("{event}");
    }
    for warning in engine.warnings() {
        eprintln!("warning: {warning}");
    }
    match result {
        Ok(()) => match engine.recompute_error() {
            // Same message style as the CLI's recompute failure output.
            Some(err) => Err(err),
            None => Ok(()),
        },
        Err(rhai_error) => Err(rhai_error.to_string()),
    }
}
