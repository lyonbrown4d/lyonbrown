# moon task graph

The root package scripts invoke the repository-local moon CLI. `pnpm dev` starts the agent TUI,
while `pnpm dev:docs` starts Astro with Starlight. The TUI task builds `agent-cli` and its complete upstream
dependency graph before executing the source entry point with the root `tsx` dependency.

Projects are represented under `.moon/projects` so their dependency edges can be maintained in one
place without coupling product packages to moon. Each project points its file group and task output
at the corresponding `apps/*` or `packages/*` directory.

Tasks use moon's `system` toolchain and the repository-provisioned Node.js 24.15 and pnpm 10.33.4.
No moon/proto Node or pnpm version management is enabled. Package commands pass
`enable-pre-post-scripts=false`; protocol generation and upstream builds are explicit graph edges,
so package lifecycle hooks cannot recursively rebuild the graph or race shared generated outputs.
The root test script uses one moon worker because integration suites in different packages bind the
same fixed loopback ports.

The normal `.moon/cache` directory is intentionally repository-local and ignored by Git. pnpm's
store and all ecosystem-wide caches remain in their existing user-level locations.
