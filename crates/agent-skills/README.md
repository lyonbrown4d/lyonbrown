# lyonbrown-skills

Rust-native file handling for the open [Agent Skills format](https://agentskills.io/specification).

Format parsing and spec validation use the maintained Govcraft
[`agent-skills` 0.2.0](https://docs.rs/agent-skills/0.2.0/) public API.
Lyonbrown adds its own manifest projection, portable-path policy and bounded
staging/archive lifecycle. It does not maintain a second frontmatter parser.
The parser adapter strips a leading UTF-8 BOM and normalizes CRLF to LF before
parsing; original installed bytes and content hashes remain unchanged. Projected
instructions therefore use LF. This upstream version accepts ASCII Skill names.

The embedding service owns authorization, downloads, durable operation intent,
receipts, events, enabled state, version selection and session bindings. This
crate grants no tool permissions and never executes Skill instructions, scripts,
npm lifecycle hooks, Node, or a package manager.

`SkillManager::open(root, limits)` receives an explicit installation root. The
caller can supply a Lyonbrown client-user root, daemon-user root or workspace
root. A user root belongs to that user on that host; this crate does not select
system-wide paths or install into another agent's directories.

## File lifecycle

1. Record the authorized operation intent in the embedding service.
2. `prepare_directory` or `prepare_archive` validates and copies into private
   staging on the same filesystem as the destination.
3. Show `PreparedSkill::inspection()` and inspect `staged_path()` as untrusted
   data. The receipt includes an ordered file inventory and content SHA-256.
4. After authorization, call `activate`. It revalidates the staged hash, rejects
   an existing target, then renames into the installation root.
5. Persist the activation receipt and publish the corresponding service event.

Dropping `PreparedSkill` removes staging. An abrupt process exit can leave
`.staging-*` directories; the embedding service reconciles these with its durable
intent records. `list()` omits these internal directories.

`quarantine(name, expected_sha256)` provides reversible removal for an explicit
uninstall or update. It checks the inspected content hash and renames the Skill
into a `.quarantine-*` directory. The returned path is retained for recovery.
The embedding service decides when to delete this recovery copy, how to restore
it, and when existing sessions release their pinned version. There is no
automatic in-place replacement or retry after an unknown result.

The engine uses an exclusive filesystem lock for activation, listing and
quarantine. Installation roots are caller-controlled private directories. Local
sources must remain unchanged during preparation; copies are checked against the
initial inventory and activation checks the prepared inventory again.

## Sources and limits

`SkillSource::parse` recognizes explicit local paths, GitHub shorthand and URLs,
and credential-free HTTPS download URLs. It performs no I/O. The embedding
service must resolve GitHub revisions to an immutable commit and apply its own
network egress, redirect, authentication, cancellation and timeout policy.

Downloads can be supplied to `prepare_archive` as a seekable reader. ZIP,
ordinary TAR and TAR.GZ are supported. TAR extensions (PAX, GNU long names,
sparse files) are rejected. Multi-Skill archives need an explicit directory
selection. A single flat archive is wrapped in the validated manifest name.
Directory installs and named archive directories must match the manifest name.

Default limits are 10 MiB archive input, 25 MiB extracted file bytes, 10 MiB per
file, 1000 filesystem entries and 16 path components. All limits are injected.
Extraction rejects links, special files, traversal, absolute paths, duplicate or
case-colliding entries and Windows-incompatible names. Markdown file links may
not escape the Skill root. Other textual instructions remain data and must
always be subject to the caller's tool and permission policy.

Manifest `allowed-tools` and `compatibility` are declarations, not permissions or
verified runtime availability. Consumers should expose compatibility as
unverified until their runtime checks have completed.
