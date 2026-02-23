# Fork Features & Merge Guide

Delta between this fork (`ozanturksever/zeroclaw`) and upstream (`zeroclaw-labs/zeroclaw`).

Net effect: −14,816 / +5,417 lines. Upstream baseline: `c47fb22`.

---

## How to Read This Document

Each section is tagged with a **merge policy**:

- 🔒 **KEEP** — Core to the fork's purpose. Must survive upstream merges. Resolve conflicts in favor of the fork.
- 🔀 **PREFER UPSTREAM** — Adopt upstream's version on next merge. Fork changes here were tactical or temporary.
- ⚖️ **NEGOTIATE** — Fork has meaningful changes but upstream may too. Manually reconcile on merge; pick the better implementation.

---

## 1. Dink Edge Mesh Integration 🔒 KEEP

The entire `src/dink/` module is new — ~2,000+ lines. It wires ZeroClaw into the OOSS platform via a Dink RPC mesh:

| Component | Purpose |
|---|---|
| `DinkRuntime` | Manages EdgeClient/CenterClient lifecycle |
| `DinkToolProvider` | Discovers remote edge services, creates Tool instances dynamically |
| `DinkServiceTool` | Wraps a single remote RPC method as a ZeroClaw `Tool` |
| `PeerMessageTool` | Inter-instance messaging via peer groups |
| `ZeroClawEdgeService` | Exposes this agent as a callable Dink service (StreamMessage, GetStatus, Shutdown, RecallMemory, UpdateConfig) |
| `DinkChannel` | A full `Channel` implementation so agents can receive messages over Dink RPC |
| `watchdog` | Connection health monitoring |
| `generated/` | Auto-generated Dink RPC types and service stubs |

Plus `ZEROCLAW_CONFIG_BASE64` env support for containerized config injection.

**Merge notes**: Entirely additive. No upstream equivalent exists. Conflicts only arise if upstream restructures `src/tools/traits.rs`, `src/channels/traits.rs`, or `src/config/schema.rs` (Dink config section).

## 2. OOSS Daemon Binary 🔒 KEEP

- New `src/bin/ooss-daemon.rs` — a minimal entrypoint that skips the full CLI and goes straight to `daemon::run()`, purpose-built for OOSS container deployments.
- New `Dockerfile.ooss` — dedicated container image for the OOSS daemon with `HEALTHCHECK`.

**Merge notes**: New files only. No conflict expected unless upstream changes `daemon::run()` signature.

## 3. Health System 🔒 KEEP

- HTTP health server (`src/health/`) for OOSS sandbox health checks, starts independently of the Dink connection.
- Heartbeat probe wired to the liveness endpoint.
- `health-check` CLI command added to `src/main.rs`.

**Merge notes**: Touches `src/main.rs` (CLI routing) and `src/health/`. If upstream adds its own health system, negotiate.

## 4. parking_lot → tokio::sync Migration ⚖️ NEGOTIATE

Complete removal of the `parking_lot` crate. All synchronization primitives migrated to `tokio::sync::Mutex` / `std::sync` equivalents. Touches ~20+ files across agents, channels, providers, memory, security, cron, gateway, tools, etc.

**Merge notes**: Wide blast radius. If upstream keeps `parking_lot`, adopting their version file-by-file and re-applying the migration is expensive. Check if upstream has independently moved away from `parking_lot`. If not, this is the biggest merge friction point.

## 5. OpenRouter SSE Streaming ⚖️ NEGOTIATE

Per-token streaming via Server-Sent Events for the OpenRouter provider (`src/providers/openrouter.rs`), replacing buffered responses.

**Merge notes**: If upstream improves OpenRouter support independently, compare implementations. Fork version is production-tested.

## 6. SOP System Removed 🔀 PREFER UPSTREAM

The entire `src/sop/` module (~5,400 lines) and its 5 associated tools (`sop_advance`, `sop_approve`, `sop_execute`, `sop_list`, `sop_status`) were deleted. This was a structured-operations-procedure engine — cut as YAGNI for the fork.

**Merge notes**: If upstream still ships SOP, let it come back on merge. The fork doesn't use it but doesn't need it gone either. Just don't wire it into OOSS paths.

## 7. MQTT Channel Removed 🔀 PREFER UPSTREAM

`src/channels/mqtt.rs` deleted entirely.

**Merge notes**: Let upstream's version return if they maintain it. No fork dependency on its absence.

## 8. Provider Simplifications ⚖️ NEGOTIATE

- OpenAI provider (`src/providers/openai.rs`) removed (109 lines).
- Gemini provider significantly trimmed (~270 lines cut) — thinking model support fixed.
- Compatible/Ollama/OpenAI-Codex providers reduced in complexity.
- Provider trait: 14 lines of trait surface removed.

**Merge notes**: The Gemini thinking-model fix is a real bugfix — push upstream or verify they've fixed it independently. OpenAI provider removal can be reverted if upstream maintains it. Provider trait changes need careful reconciliation — check if fork's trait is a subset of upstream's.

## 9. Channel Simplifications ⚖️ NEGOTIATE

- Discord, Lark, Telegram channels significantly reduced (Discord: −347 lines, Telegram: −700+ lines net rewrite, Lark: −244 lines).
- Channel factory (`src/channels/mod.rs`) trimmed.

**Merge notes**: Fork simplifications may drop features upstream users depend on. On merge, prefer upstream channel implementations unless they conflict with Dink channel wiring. The Telegram `mention_only` mode (section 11) should be preserved regardless.

## 10. Security Changes ⚖️ NEGOTIATE

- Legacy XOR cipher (`enc:`) deprecated → auto-migration to ChaCha20-Poly1305 (`enc2:`) via `decrypt_and_migrate()`.
- Pairing module trimmed (~83 lines).
- Security policy simplified (~159 lines cut).

**Merge notes**: The `enc:` → `enc2:` migration is a security improvement — check if upstream adopted it. If not, keep fork's version. Pairing/policy trims may lose upstream hardening; review diff carefully on merge.

## 11. Config/Schema Changes 🔒 KEEP (Dink section) / ⚖️ NEGOTIATE (rest)

- `src/config/schema.rs`: Dink config section added **(KEEP)**, defaults adjusted, schema trimmed.
- Default gateway port changed to `42617` (was `3000`) — **NEGOTIATE**, upstream may have its own default.
- Telegram `mention_only` mode added — **KEEP**.
- OpenRouter set as default provider — **NEGOTIATE**, fork preference only.

**Merge notes**: The Dink config struct and Telegram `mention_only` field must survive. Other default changes can yield to upstream.

## 12. Agent Core Changes 🔒 KEEP (Dink wiring) / ⚖️ NEGOTIATE (simplifications)

- `src/agent/agent.rs`: `memory_ref()` getter and Dink-aware wiring **(KEEP)**.
- `src/agent/loop_.rs`: ~280 lines cut — simplified orchestration loop **(NEGOTIATE)**.
- Classifier and dispatcher trimmed **(NEGOTIATE)**.

**Merge notes**: The `memory_ref()` getter and Dink integration points in the agent must be preserved or re-applied. Loop/classifier/dispatcher simplifications can yield to upstream if they've evolved those paths.

## 13. Build/CI/Docs 🔀 PREFER UPSTREAM (CI) / 🔒 KEEP (fork release script)

- Homebrew publish workflow removed — **PREFER UPSTREAM** (let it return).
- Release workflow trimmed — **PREFER UPSTREAM**.
- `scripts/release/fork_release.sh` added — **KEEP**.
- `examples/dink_config.toml` added — **KEEP**.
- Docs pruned: SOP docs, macOS update/uninstall guide, release process, structure docs removed — **PREFER UPSTREAM**.

**Merge notes**: Fork-specific files (`fork_release.sh`, `dink_config.toml`, `Dockerfile.ooss`) are additive. Let upstream docs and CI workflows return as-is.

## 14. Dependency Changes 🔒 KEEP (dink-sdk) / 🔀 PREFER UPSTREAM (rest)

- `dink-sdk` added (now at `0.3.1` from crates.io) — **KEEP**.
- `parking_lot` removed — see section 4.
- `which` bumped 7→8, `rppal` bumped 0.19→0.22 — **PREFER UPSTREAM** (take whatever version upstream uses).

---

## Merge Checklist

Before merging upstream:

1. **Identify upstream baseline**: compare against the commit noted above (`c47fb22`).
2. **KEEP items**: re-apply or conflict-resolve in favor of the fork.
3. **PREFER UPSTREAM items**: accept upstream's version; drop fork-only changes.
4. **NEGOTIATE items**: diff both versions, pick the better one, document the choice.
5. **Test Dink integration** end-to-end after merge — it touches agent, channels, tools, config, and main.
6. **Verify `parking_lot` state**: if upstream still uses it, decide whether to re-migrate or defer.
7. **Run full validation**: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test`.
