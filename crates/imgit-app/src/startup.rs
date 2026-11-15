//! Opt-in startup tracing. Records in memory, then writes once after the window is shown.
//! Set `IMGIT_STARTUP_TIMINGS` to an output file path. No repository paths are recorded.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static TRACE: OnceLock<Mutex<Trace>> = OnceLock::new();

struct Trace {
    start: Instant,
    path: PathBuf,
    rows: Vec<(&'static str, f64, f64)>,
    finished: bool,
}

/// Call once from main, after excluding credential-helper invocations.
pub fn init(start: Instant) {
    if let Some(path) = std::env::var_os("IMGIT_STARTUP_TIMINGS").filter(|p| !p.is_empty()) {
        let _ = TRACE.set(Mutex::new(Trace {
            start,
            path: path.into(),
            rows: Vec::new(),
            finished: false,
        }));
    }
}

/// A measured scope; inactive when tracing is disabled or startup has finished.
pub struct Span(Option<(&'static str, Instant)>);

pub fn span(name: &'static str) -> Span {
    Span(TRACE.get().and_then(|trace| {
        let trace = trace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (!trace.finished).then(|| (name, Instant::now()))
    }))
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some((name, start)) = self.0 {
            record(name, start);
        }
    }
}

fn record(name: &'static str, start: Instant) {
    let end = Instant::now();
    if let Some(trace) = TRACE.get() {
        let mut trace = trace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !trace.finished {
            let elapsed = end.duration_since(trace.start).as_secs_f64() * 1000.0;
            trace.rows.push((
                name,
                elapsed,
                end.duration_since(start).as_secs_f64() * 1000.0,
            ));
        }
    }
}

/// A zero-duration milestone; elapsed time is relative to entry into main.
pub fn mark(name: &'static str) {
    record(name, Instant::now());
}

/// Flush once. Errors are diagnostic only and must not prevent startup.
pub fn finish() {
    let Some(trace) = TRACE.get() else { return };
    let mut trace = trace
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if trace.finished {
        return;
    }
    trace.finished = true;
    let result = (|| -> std::io::Result<()> {
        let mut file = std::io::BufWriter::new(std::fs::File::create(&trace.path)?);
        writeln!(file, "phase,elapsed_ms,duration_ms")?;
        for (name, elapsed, duration) in &trace.rows {
            writeln!(file, "{name},{elapsed:.3},{duration:.3}")?;
        }
        file.flush()
    })();
    if let Err(error) = result {
        eprintln!("imgit: could not write startup timings: {error}");
    }
}
