# Reusable decision engine

`decision-core` is an embedded Rust library for advice and constraint validation. It owns no Agent Session, database, provider credentials or HTTP server. The Agent embeds it by default; the independent [AI Gateway](gateway.md) consumes the same API. Callers retain actual dispatch, permissions and durable state.

## Operations and results

| Operation | Output |
| --- | --- |
| `RouteModel` | Task domain, complexity, tier, required capabilities and candidate model keys |
| `Classify` | A caller-supplied label and scored alternatives |
| `Predicate` | Boolean judgment and confidence |
| `Choice` | A supplied option and distribution |
| `Score` | A value within the supplied scale |
| `RecommendTools` | Candidates from the authorized tool descriptions |
| `ContextAdvice` | Compaction advice and remaining token capacity |

Requests carry an ID, operation, constraints, optional exact policy revision and deadline. Results distinguish `Accepted`, `Unknown`, `Degraded` and `Denied`; Unknown means neither false, low risk nor permission. Provenance records policy, backend and index versions, reason codes and elapsed time. Confidence and similarity cannot authorize execution.

```rust
use decision_core::*;

# async fn example() -> Result<(), PolicyError> {
let engine = DecisionEngine::new(Policy::default(), Vec::new())?;
let request = DecisionRequest::new("example", Operation::Classify(ClassifyInput {
  text: "Review Rust code".into(),
  labels: vec!["coding".into(), "chat".into()],
}));
let result = engine.decide(&request, &DecisionCancellation::new()).await;
# Ok(())
# }
```

Run the standalone example with `cargo run --locked -p decision-core --no-default-features --example embedded`. The default build loads no weights, performs no networking and has no Agent Runtime dependency.

## Ordering and constraints

The engine validates input, policy revision and caller-authorized model metadata before handling explicit/pinned models, rules, configured backends and constrained fallback. Rules use configured terms and abstain when multiple domains match. Neither user text nor classifier output can add authorized candidates.

Local-only requirements, model-key/tier restrictions, capabilities, context capacity and per-call estimated cost are checked before returning output. Unknown capacity or pricing cannot satisfy a corresponding limit. Integer micro-USD estimates are routing inputs, not billing. An invalid explicit or pinned model is denied rather than silently replaced.

Rules can directly calculate context advice and match authorized tool descriptions. Semantic and structured backends provide other judgments; missing backends return Unknown. Failures, low confidence, invalid output and timeouts retain reasons; cancellation stops waiting. Conservative model fallback remains inside eligible caller-owned candidates.

`replace_policy` validates a complete immutable snapshot before publishing it atomically. Each request uses one snapshot; changed content requires a new revision. Task bindings remain caller-owned and enter through `pinned_model`.

## Semantic and structured backends

`decision-semantic` uses a replaceable `EmbeddingProvider`, versioned examples and hard negatives. It validates dimensions and finite values and applies score and margin thresholds rather than forcing a winner. Hosts can supply embeddings without a native runtime. The optional `semantic-local` feature provides a `fastembed` adapter.

The local adapter accepts explicit files with SHA-256 checks: ONNX model, tokenizer assets and ONNX Runtime library. Deployment prepares and licenses these assets; loading never contacts a model hub. ONNX Runtime is process-global; the host must use consistent runtime-library configuration and share its first initialized environment. Loading and inference run on blocking workers, with concurrency permits retained until actual computation ends even after the caller cancels. Model, preprocessing, corpus and threshold versions contribute to index provenance.

`decision-model` adapts remote models or an existing local `InferenceBackend` to typed judgments. Shared `model-provider` adapters make one bounded request without a Harness, tools or automatic retries. Output must match the operation, supplied labels/options and numeric bounds. Local-only requests exclude remote classifiers. This uses existing model integration without requiring an additional Candle judgment model.

## Agent and Gateway integration

`agent-decision` retains Agent-specific tool, node and approval policy. Each Run calls the shared core by default, pins the configured main model and persists `model_route_decision` before the effective tool plan and Harness startup. The record audits that Run’s explicit main-model binding. Advice cannot bypass workspace limits, paired-worker scope or human approval.

Gateway `router/auto` passes its configured model catalog to the same core, selects a physical model and rechecks constraints. Neither consumer owns the other's Session or requires the other service to run.

## Validation scope

Core tests cover rules, typed output, hard constraints, cancellation, backend failure, policy replacement and fixed Chinese/English replay fixtures. Deterministic semantic tests inject synthetic vectors; they validate routing behavior, not real multilingual model accuracy. Real local-model tests require explicit prepared assets; see [Development and validation](development.md).

On October 19, 2026, macOS arm64 acceptance used explicit, pinned multilingual MiniLM ONNX and ONNX Runtime assets: four known Chinese/English inputs classified correctly and two unknown inputs were rejected. Calibration and replay fixtures are disjoint. This is small-fixture adapter acceptance rather than a language-quality or performance benchmark. The [acceptance record](evidence/decision-semantic-offline.json) lists versions, thresholds and scope.
