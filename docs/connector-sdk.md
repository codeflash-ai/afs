# Connector SDK

A connector implements five responsibilities and leaves caching, journaling, validation, conflict detection, and rate limiting to the host:

1. Enumerate remote tree metadata.
2. Fetch full native content for one entity.
3. Render native content to canonical Markdown plus frontmatter.
4. Parse edited canonical content back to a connector-owned model.
5. Apply a validated push plan as remote API operations.

First-party connectors compile in as Rust crates. A future third-party connector ABI should be possible if this trait remains narrow, explicit, and host-mediated.

## v1 connector

`afs-notion` is the first connector. It owns Notion-specific block mapping, database schema translation, OAuth/API behavior, and conversion between Notion payloads and the canonical AgentFS document model.

