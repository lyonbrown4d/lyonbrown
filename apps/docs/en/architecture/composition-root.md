# FluxDI Composition Root

Lyonbrown pins `FluxDI 1.2.2` as the repository-wide composition skeleton. It wires objects without spreading dependency injection into the domain model: domain crates such as `agent-core`, `agent-model`, `agent-tool`, `agent-secret`, `agent-profile`, and `agent-storage` continue to express dependencies through constructors, traits, and explicit parameters and do not depend on FluxDI. The CLI and GPUI GUI own separate composition roots. A future server or other application entry point should use the same boundary rather than make domain crates container-aware.

## Composition boundary

Startup has two phases:

1. `CoreModule` binds the already resolved `RuntimeConfig`, `RuntimePaths`, and `ApprovalPolicy`.
2. After external resources are prepared asynchronously, `RuntimeModule` binds the model and tool registries, Secret Broker, Agent/Workspace Profiles, Skill Registry, optional Storage, optional Task Router, and the final `KernelFactory`.

The GUI `GuiModule` binds `CockpitState` and `CockpitFactory`, then validates its graph before opening a window. Both applications expose typed entry points such as `build_kernel()` or `create_cockpit()` instead of a generic container resolver.

```text
configuration + paths + approval policy
                  │
                  ▼
          FluxDI CompositionRoot
                  │
                  ├── ModelRegistry / TaskRouter
                  ├── ToolRegistry / SecretBroker
                  ├── AgentProfile / WorkspaceProfile / SkillRegistry
                  ├── SessionStore (optional)
                  └── KernelFactory ──► Kernel
```

`KernelFactory` is the narrow adapter between the container and the domain builder. It derives `RoutingPolicy` from the Profile and passes configuration, registries, approval, secrets, profiles, storage, and routing to `Kernel::builder()`. The Kernel neither parses configuration nor resolves dependencies from a global container.

## Dependency-graph validation

The composition root calls `try_validate_graph()` after all asynchronous services are registered. This validation detects missing bindings, cycles, and incomplete `KernelFactory` dependencies before the CLI accepts a session. Every new root-level service should:

- be registered by an application module;
- declare Provider edges with `with_dependency`;
- be covered by `CompositionRoot::validate()` and composition-root tests.

Startup can still fail while opening a model file, network resource, or database. Graph validation verifies composition relationships; it does not replace resource health checks.

## Override and replacement strategy

Replacement happens at composition-root inputs, not through a service locator in domain crates:

- Configuration is overlaid in the order “built-in defaults → global file → trusted workspace file → `--config` → environment → CLI flags,” then one `RuntimeConfig` instance is bound.
- Paths and approval policy are supplied by the caller, so tests can use temporary directories and a `SessionApprovalPolicy` without modifying domain code.
- Providers, tools, the Secret Broker, profiles, storage, and the Task Router are constructed by `prepare_services` and then bound as interfaces or registries. Alternative implementations belong at this application-layer seam.
- Optional capabilities use explicit wrapper bindings, keeping “not registered” distinct from “configured off.”

The production root should not hot-swap singletons after startup. Tests that need different implementations should create an isolated root. A new application entry point should reuse the domain construction APIs while owning its own modules and lifecycle.
