# `@lyonbrown/decision-engine`

Daemon-owned typed decision port and OpenJev adapter. Public APIs contain only Lyonbrown-owned
types. Provider SDK values, model paths, and runtime objects stay inside the composition adapter.

The daemon configuration boundary must inject a fully qualified local decision-model storage root
through `LYONBROWN_DECISION_MODEL_ROOT`. Local `.env` files are ignored; `.env.example` documents
the contract. A missing or invalid value leaves the decision provider unavailable and never falls
back to a system drive. Windows paths must include a drive and cannot use UNC or device namespaces.
