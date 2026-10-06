# TUI Local Real-Scenario Black-Box Validation

> Status: Core TUI passed; production tool, approval, and model-selection blockers closed
> Validation date: 2026-10-07
> Environment: Windows PowerShell, local PTY, Node.js 24, real `agent` build, real local daemon

## 1. Scope

The run started `apps/agent-cli/dist/main.js` through the user-facing executable
path, operated the TUI in a real PTY, and connected to the authenticated daemon
discovered or spawned by the product launcher. Model transfer, GGUF loading,
inference, Snapshot updates, and Session recovery used production code rather
than test doubles.

Typed render regressions separately cover designed approval and rich Inspector
states. A scenario counts as a black-box pass here only when it traverses the
production daemon. Design renders are not runtime evidence for this document.

## 2. Real-scenario results

| Scenario                    | Result       | Observed evidence                                                                                                                                              |
| --------------------------- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Startup and connection      | Pass         | The TUI appeared first and then connected to the authenticated daemon; model transfer did not block startup.                                                   |
| Missing model               | Pass         | The daemon recommended a model without forcing a download; first-run auto recommendation is capped by a 1 GiB download budget.                                 |
| English/Chinese switch      | Pass         | `F2` changed the model-setup copy immediately between Simplified Chinese and English.                                                                          |
| Background download         | Pass         | After explicit `D`, the UI showed WaitingForLock, Downloading 0–100%, Verifying, Activating, and Ready while remaining responsive.                             |
| Real local inference        | Pass         | An exact-response prompt returned `READY`; activity usage came from a real Snapshot.                                                                           |
| Live steering               | Pass         | Plain Enter during an active turn sent steering and the following output obeyed the narrower instruction.                                                      |
| Follow-up queue             | Pass         | `Alt-Enter` displayed the queued item, which became the next turn after the active turn completed.                                                             |
| Activity Inspector          | Pass         | `Ctrl-O` showed freshness, activities, route, token usage, and assistant evidence, then closed cleanly.                                                        |
| Theme control panel         | Pass         | `Ctrl-P` search opened the panel; all six themes appeared; Space preview kept saved and preview values distinct; Escape restored the saved theme.              |
| Preference persistence      | Pass         | Slate Blue survived a clean exit and restart, then could be saved back to the default Hearth Teal.                                                             |
| Session recovery            | Pass         | Restart reached Ready with the cached model and restored the existing transcript.                                                                              |
| 80-column response          | Pass         | The model stayed in the context line, long output wrapped without loss and used one Agent label per response, and narrow control actions remained readable.    |
| Clean exit                  | Pass         | `/quit` returned exit code 0; exiting did not implicitly cancel an accepted daemon download.                                                                   |
| Tool call and real approval | Layered pass | A real GGUF emitted `write`, the daemon projected approval, and client `allow-once` produced the exact file; TUI approval rendering and keys have regressions. |
| Session model switch        | Layered pass | A real client pinned and restored auto selection through the daemon; the TUI sends only daemon-advertised routes.                                              |

## 3. Closed production blockers

The 2026-10-07 production-composition real-GGUF smoke closed the original blockers:

- production injects Pi Coding Agent `read`, `write`, and `edit` workspace tools;
  `@openclaw/fs-safe` runs in native-required mode with rooted, identity-checked
  I/O, atomic writes, a 1 MiB file cap, and fail-closed link/race handling;
- the local-model adapter carries typed tool definitions, assistant tool calls,
  and tool-result history;
- exact approval decisions, model selection, and tool results become Lyonbrown
  domain events before gRPC projection; session approval policy is replayable
  instead of existing only in memory.

The real smoke covers explicit download, cancel/retry, digest verification,
activation, model selection, a short answer, approved `write` and exact file
content, turn cancellation, cached restart, and bounded shutdown, ending in
`REAL_GGUF_SMOKE_OK`. Evidence combines the prior real PTY run, TUI automation,
and a production C/S black box; a design render or fixture alone is not treated
as end-to-end evidence.

## 4. Automated regression complement

Automation covers the main surface, model setup, command palette, six-theme
control panel, bilingual status, inline approval, Inspector evidence,
narrow-terminal wrapping, steering/queue/cancel command boundaries, preference
file validation, and terminal text safety. The real black-box run and automated
regressions together form the current evidence; failure at either layer prevents
a claim that the production flow is complete.
