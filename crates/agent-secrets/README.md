# Agent Secrets

`agent-secrets` provides host-local encrypted Provider credential files. An
authenticated service supplies a dedicated root and owns upload consent,
operation intents, configuration references, receipts and audit events.

```rust,no_run
use agent_secrets::{SecretStore, SecretString};

# fn example() -> agent_secrets::Result<()> {
let store = SecretStore::open("/trusted/user-data/provider-secrets")?;
let key = SecretString::from("synthetic-example-key");
let metadata = store.put("provider-openai", &key)?;
let credential = store.get(&metadata.id)?;
# Ok(())
# }
```

The binary record format uses XChaCha20-Poly1305 with a random 192-bit nonce per
write. Authenticated data binds the record to its credential ID and format
version. Values are bounded to 16 KiB. Metadata contains only the supplied ID
and encryption version. IDs use lowercase ASCII letters, digits, hyphens and
underscores, with a 64-character limit and Windows reserved names excluded.
Plaintext uses `secrecy`/`zeroize` wrappers and has no
serialization API. Changes use a process lock and a synced temporary file
followed by an atomic replacement. A damaged existing record cannot be
overwritten or removed through this API before its owner reconciles it.

On Unix, the dedicated directory uses mode `0700`, and master key, lock and
record files use `0600`. Group/other-accessible key or record files are rejected,
as are symlink records. On Windows, directory/file ACLs inherit from the supplied
data directory; this crate does not configure or verify Windows ACLs. The host
must provision a private user data directory before exposing credential upload.

The master key is stored beside ciphertext and is protected by filesystem
access, rather than an OS keychain or hardware security module. Copying the
entire directory copies the decryption key. This does not protect against the
owning OS user, a compromised host, memory inspection, or storage rollback.
Deletion removes the named encrypted file and does not promise forensic erasure.
No migration or master-key rotation is provided in this version.

The supplied root and its ancestors are trusted host paths. Callers should keep
secret IDs opaque, avoid displaying plaintext in error/debug handling, use
authenticated encrypted transport, and keep secret values out of Session,
ViewModel, tools, model context and logs.
