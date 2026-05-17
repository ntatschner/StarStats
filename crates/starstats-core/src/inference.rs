//! Post-classify pass that emits inferred events from surrounding context.
//!
//! Rules are pure-Rust structs in v1, mirroring the BurstRule / RemoteRule
//! style: declarative, append-only, no learned components. The inference
//! pass is pure — given the same input event slice it produces the same
//! output, which keeps tests deterministic and idempotent.

use crate::events::GameEvent;
use crate::metadata::EventMetadata;
use crate::wire::EventEnvelope;

/// Tunables for the inference pass. The defaults match the design spec
/// (Phase 3); callers needing tighter bounds (e.g. unit tests probing
/// edge cases) can override.
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    /// Hard cap on the forward / backward scan distance in events. Acts
    /// as a defence-in-depth bound on top of the per-rule wall-clock
    /// limits so a malformed log can't make a rule walk the whole stream.
    pub window_size: usize,
    /// Reconciliation window in seconds. An inferred event is marked
    /// `superseded_by` the first observed event of matching
    /// `(event_type, primary_entity)` that lands within this window
    /// after the inferred timestamp.
    pub reconciliation_secs: i64,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            window_size: 200,
            reconciliation_secs: 5,
        }
    }
}

/// One result row from [`infer`]. Carries the synthesised event, its
/// metadata, and a back-reference to the observed event whose
/// classification triggered the rule.
#[derive(Debug, Clone, PartialEq)]
pub struct InferredEvent {
    pub event: GameEvent,
    pub metadata: EventMetadata,
    /// The observed event whose classification triggered this inference.
    /// Used for idempotency_key derivation and audit trail.
    pub trigger_idempotency_key: String,
    /// If this inferred event was later superseded by an actual observed
    /// event of the same `(event_type, primary_entity)` within the
    /// reconciliation window, this is the observed event's idempotency_key.
    /// Timeline consumers drop superseded rows but storage retains them.
    pub superseded_by: Option<String>,
}

/// Run all inference rules over the event stream. Pure function:
/// given the same input it returns the same output.
pub fn infer(events: &[EventEnvelope], config: &InferenceConfig) -> Vec<InferredEvent> {
    let _ = (events, config);
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_on_empty_stream_returns_no_inferences() {
        assert!(infer(&[], &InferenceConfig::default()).is_empty());
    }
}
