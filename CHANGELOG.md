# Changelog

All notable changes to ZeroClaw will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-02-23 (fork)

### Fork Changes

- 458526c docs: update FORKSTATE-GUIDE baseline to 359cfb4 post-merge
- 72b2cf3 merge(upstream): sync upstream/dev into fork
- ae66a8d docs: add forkstate guide documenting merge policies and changes
- 4633217 build(ooss): add ooss-daemon binary, HEALTHCHECK, dink-sdk 0.3.1
- debf044 deps: switch dink-sdk from path to crates.io v0.3.1

### Docs / CI Changes

- 72b2cf3 merge(upstream): sync upstream/dev into fork

### Upstream Baseline

- upstream/main: c47fb22


## [0.2.0] - 2026-02-23 (fork)

### Fork Changes

- 48504fe feat: health endpoint liveness wiring, heartbeat probe, health-check CLI
- 57b2ffd fix: complete parking_lot → tokio::sync/std::sync migration
- 76c2e72 refactor: eliminate parking_lot — full migration to tokio::sync::Mutex
- ebfd16b refactor: dink-sdk 0.2.0, typed service handler, parking_lot → tokio mutex
- 83e95ec fix: unwrap Dink SDK payload envelope in edge service handler
- 8748491 fix: start health server independently of Dink connection
- deddc54 feat: add health HTTP server for OOSS sandbox health checks
- 54bc124 fix: suppress unused variable warning in agent channel loop
- 253a588 feat: live GetStatus metrics + Shutdown handler
- 5ba5e2a feat: per-token streaming via OpenRouter SSE
- 539cc26 feat: StreamMessage RPC with real streaming, UpdateConfig live reload
- 88a4ce7 feat: wire RecallMemory RPC to live memory backend, add memory_ref() getter
- 3dbceb1 feat: Dink edge integration, ZEROCLAW_CONFIG_BASE64 env support, OpenRouter default
- e326701 chore(deps): bump which from 7.0.3 to 8.0.0
- d7d5ac3 chore(deps): bump rppal from 0.19.0 to 0.22.1
- e5a6ab4 chore(deps): bump the rust-all group with 4 updates

### Docs / CI Changes

- 9fd9bc8 chore(deps): bump github/codeql-action in the actions-all group

### Upstream Syncs

- 1a65766 Merge remote-tracking branch 'upstream/dev'

### Upstream Baseline

- upstream/main: c47fb22


### Security
- **Legacy XOR cipher migration**: The `enc:` prefix (XOR cipher) is now deprecated. 
  Secrets using this format will be automatically migrated to `enc2:` (ChaCha20-Poly1305 AEAD)
  when decrypted via `decrypt_and_migrate()`. A `tracing::warn!` is emitted when legacy
  values are encountered. The XOR cipher will be removed in a future release.

### Added
- `SecretStore::decrypt_and_migrate()` — Decrypts secrets and returns a migrated `enc2:` 
  value if the input used the legacy `enc:` format
- `SecretStore::needs_migration()` — Check if a value uses the legacy `enc:` format
- `SecretStore::is_secure_encrypted()` — Check if a value uses the secure `enc2:` format
- **Telegram mention_only mode** — New config option `mention_only` for Telegram channel.
  When enabled, bot only responds to messages that @-mention the bot in group chats.
  Direct messages always work regardless of this setting. Default: `false`.

### Deprecated
- `enc:` prefix for encrypted secrets — Use `enc2:` (ChaCha20-Poly1305) instead.
  Legacy values are still decrypted for backward compatibility but should be migrated.

### Fixed
- **Gemini thinking model support** — Responses from thinking models (e.g. `gemini-3-pro-preview`)
  are now handled correctly. The provider skips internal reasoning parts (`thought: true`) and
  signature parts (`thoughtSignature`), extracting only the final answer text. Falls back to
  thinking content when no non-thinking response is available.
- Updated default gateway port to `42617`.
- Removed all user-facing references to port `3000`.
- **Onboarding channel menu dispatch** now uses an enum-backed selector instead of hard-coded
  numeric match arms, preventing duplicated pattern arms and related `unreachable pattern`
  compiler warnings in `src/onboard/wizard.rs`.
- **OpenAI native tool spec parsing** now uses owned serializable/deserializable structs,
  fixing a compile-time type mismatch when validating tool schemas before API calls.

## [0.1.0] - 2026-02-13

### Added
- **Core Architecture**: Trait-based pluggable system for Provider, Channel, Observer, RuntimeAdapter, Tool
- **Provider**: OpenRouter implementation (access Claude, GPT-4, Llama, Gemini via single API)
- **Channels**: CLI channel with interactive and single-message modes
- **Observability**: NoopObserver (zero overhead), LogObserver (tracing), MultiObserver (fan-out)
- **Security**: Workspace sandboxing, command allowlisting, path traversal blocking, autonomy levels (ReadOnly/Supervised/Full), rate limiting
- **Tools**: Shell (sandboxed), FileRead (path-checked), FileWrite (path-checked)
- **Memory (Brain)**: SQLite persistent backend (searchable, survives restarts), Markdown backend (plain files, human-readable)
- **Heartbeat Engine**: Periodic task execution from HEARTBEAT.md
- **Runtime**: Native adapter for Mac/Linux/Raspberry Pi
- **Config**: TOML-based configuration with sensible defaults
- **Onboarding**: Interactive CLI wizard with workspace scaffolding
- **CLI Commands**: agent, gateway, status, cron, channel, tools, onboard
- **CI/CD**: GitHub Actions with cross-platform builds (Linux, macOS Intel/ARM, Windows)
- **Tests**: 159 inline tests covering all modules and edge cases
- **Binary**: 3.1MB optimized release build (includes bundled SQLite)

### Security
- Path traversal attack prevention
- Command injection blocking
- Workspace escape prevention
- Forbidden system path protection (`/etc`, `/root`, `~/.ssh`)

[0.1.0]: https://github.com/theonlyhennygod/zeroclaw/releases/tag/v0.1.0
