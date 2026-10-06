# TUI Local Real-Scenario Black-Box Validation

> Status: Partially passed; production tool path blocked
> Validation date: 2026-10-06
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

| Scenario                    | Result      | Observed evidence                                                                                                                                           |
| --------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Startup and connection      | Pass        | The TUI appeared first and then connected to the authenticated daemon; model transfer did not block startup.                                                |
| Missing model               | Pass        | It recommended `qwen3-1.7b-q4_k_m` (about 1.03 GiB) without forcing a download.                                                                             |
| English/Chinese switch      | Pass        | `F2` changed the model-setup copy immediately between Simplified Chinese and English.                                                                       |
| Background download         | Pass        | After explicit `D`, the UI showed WaitingForLock, Downloading 0–100%, Verifying, Activating, and Ready while remaining responsive.                          |
| Real local inference        | Pass        | An exact-response prompt returned `READY`; activity usage came from a real Snapshot.                                                                        |
| Live steering               | Pass        | Plain Enter during an active turn sent steering and the following output obeyed the narrower instruction.                                                   |
| Follow-up queue             | Pass        | `Alt-Enter` displayed the queued item, which became the next turn after the active turn completed.                                                          |
| Activity Inspector          | Pass        | `Ctrl-O` showed freshness, activities, route, token usage, and assistant evidence, then closed cleanly.                                                     |
| Theme control panel         | Pass        | `Ctrl-P` search opened the panel; all six themes appeared; Space preview kept saved and preview values distinct; Escape restored the saved theme.           |
| Preference persistence      | Pass        | Slate Blue survived a clean exit and restart, then could be saved back to the default Hearth Teal.                                                          |
| Session recovery            | Pass        | Restart reached Ready with the cached model and restored the existing transcript.                                                                           |
| 80-column response          | Pass        | The model stayed in the context line, long output wrapped without loss and used one Agent label per response, and narrow control actions remained readable. |
| Clean exit                  | Pass        | `/quit` returned exit code 0; exiting did not implicitly cancel an accepted daemon download.                                                                |
| Tool call and real approval | **Blocked** | The model received no available tools, reported zero tool activity, and could not produce an approval through production.                                   |
| Session model switch        | **Blocked** | The control UI exists, but production does not support `select-session-model`.                                                                              |

## 3. Blocking cause

The tool/approval failure is not a missing TUI renderer:

- `apps/agentd/node-src/production-kernel.ts` currently composes production with
  an empty tool registry and default-denied tool permission;
- `apps/agentd/node-src/local-model-adapter.ts` explicitly rejects non-empty
  tools, tool-call messages, and tool-result messages;
- `apps/agentd/node-src/service.ts` does not yet implement `respond-approval` or
  `select-session-model`.

This validation therefore does not relabel fixture-rendered approval or tool
evidence as a real black-box pass. Closing the two blocked rows requires a real
daemon tool registry, model tool-calling protocol, fail-closed approval gate,
and the corresponding gRPC commands, followed by the same PTY run.

## 4. Automated regression complement

Automation covers the main surface, model setup, command palette, six-theme
control panel, bilingual status, inline approval, Inspector evidence,
narrow-terminal wrapping, steering/queue/cancel command boundaries, preference
file validation, and terminal text safety. The real black-box run and automated
regressions together form the current evidence; failure at either layer prevents
a claim that the production flow is complete.
