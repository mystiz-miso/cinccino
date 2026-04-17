//! `tracing_subscriber::Layer` that emits WARN for any span whose
//! lifetime exceeds a configured threshold. Makes pathological LSP
//! requests (proc-macro expansion blow-ups, large-file formatting,
//! etc.) visible in the indexer's captured stderr without needing a
//! separate profiler.
//!
//! Timing is wall-clock from span creation to span close — *not*
//! busy-time across enter/exit pairs. For an async LSP handler this
//! means the reported "elapsed" includes awaits on I/O, scheduler
//! gaps, and any other wall-clock latency the caller actually waits
//! for. That matches what a user perceives as "this request is slow"
//! and is the right thing to escalate; splitting busy vs idle would
//! under-report genuinely slow requests whose cost is dominated by
//! awaiting.

use std::time::{Duration, Instant};

use tracing::Subscriber;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

pub struct SlowRequestLayer {
    threshold: Duration,
}

impl SlowRequestLayer {
    pub fn new(threshold: Duration) -> Self {
        Self { threshold }
    }
}

impl<S> Layer<S> for SlowRequestLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        _attrs: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        ctx: Context<'_, S>,
    ) {
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Instant::now());
        }
    }

    fn on_close(&self, id: tracing::Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let elapsed = {
            let ext = span.extensions();
            match ext.get::<Instant>() {
                Some(start) => start.elapsed(),
                None => return,
            }
        };
        if elapsed >= self.threshold {
            tracing::warn!(
                target: "cinccino::slow",
                span = span.name(),
                elapsed_ms = elapsed.as_millis() as u64,
                "slow LSP request",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::thread::sleep;
    use tracing::{info_span, subscriber::with_default};
    use tracing_subscriber::fmt::MakeWriter;
    use tracing_subscriber::layer::SubscriberExt;

    #[derive(Clone, Default)]
    struct BufWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for BufWriter {
        type Writer = Self;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    impl std::io::Write for BufWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn subscribed<F: FnOnce()>(threshold: Duration, f: F) -> String {
        let buf = BufWriter::default();
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(buf.clone())
            .with_target(true)
            .with_ansi(false);
        let subscriber = tracing_subscriber::registry()
            .with(fmt_layer)
            .with(SlowRequestLayer::new(threshold));
        with_default(subscriber, f);
        let bytes = buf.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn warns_when_span_exceeds_threshold() {
        let out = subscribed(Duration::from_millis(5), || {
            let _span = info_span!("hover").entered();
            sleep(Duration::from_millis(20));
        });
        assert!(
            out.contains("slow LSP request") && out.contains("hover"),
            "expected WARN for slow span, got: {out}",
        );
    }

    #[test]
    fn silent_when_span_under_threshold() {
        // Threshold of 10 minutes — nothing we do here trips it.
        let out = subscribed(Duration::from_secs(600), || {
            let _span = info_span!("hover").entered();
            sleep(Duration::from_millis(1));
        });
        assert!(
            !out.contains("slow LSP request"),
            "unexpected slow-request log under threshold: {out}",
        );
    }
}
