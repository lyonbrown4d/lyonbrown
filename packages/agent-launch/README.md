# @lyonbrown/agent-launch

Pure orchestration for discovering, reusing, starting, and authenticating the local Lyonbrown
daemon. The package owns no operating-system security implementation and does not provide an
embedded fallback.

The controlled `@lyonbrown/agent-launch/composition` entry is only for trusted client composition
roots. It receives the native control-plane capability plus consumer-owned discovery, startup-lease,
process, clock, sleep, and authenticated client adapters.
Discovery returns an opaque capability whose implementation privately retains connection material.
The exported client entry contains only connection input/result types and safe launch errors; it
does not expose composition ports or construction details.

The coordinator authenticates every candidate and requires the discovery identity, handshake
identity, and configuration-derived scope to agree before reuse. Exactly one startup-lease winner
starts the daemon; other clients wait and authenticate the published candidate. A daemon epoch
change causes client rebinding and snapshot resynchronization. The launcher never submits or replays
agent commands.

Process launch input contains only the canonical workspace, effective configuration fingerprint,
and derived scope. Credential generation and secure publication remain daemon and native control
plane responsibilities.

The process adapter returns a `SpawnAttempt` before it can commit a process. Failed attempts enter
background quarantine: caller cancellation remains bounded, while the startup lease stays held
until the attempt explicitly retires or its process exits. This prevents another client from
starting a duplicate daemon while abandonment is rejected or still pending.
