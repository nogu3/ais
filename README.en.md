# ais

[![CI](https://github.com/nogu3/ais/actions/workflows/ci.yml/badge.svg)](https://github.com/nogu3/ais/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

日本語: [README.md](README.md)

A CLI dedicated to the Panasonic **AiSEG2** HEMS controller. It talks to the AiSEG2 web UI over HTTP (Digest auth) to read power data and to control devices that can only be automated through AiSEG2 (such as Link Plus lighting).

- stdout carries **pure structured JSON only** (directly consumable by `jq` and LLM function calling)
- Diagnostics go to stderr as structured logs
- Stateless / one-shot (no daemon, no polling loop)

Intended to be invoked as a binary on `PATH` by callers such as cron, n8n, or dashboards.

> AiSEG2 is a Japan-only product, so the primary documentation is the Japanese [README.md](README.md). This page summarizes the essentials.

## Target firmware

AiSEG2 production ends in September 2026 with no further firmware updates, so the HTML / control-endpoint contract is treated as **frozen**.

**Verified firmware: `Ver.2.97M-03`**. All subcommands (reads, lighting control, exit-code contract) have been verified against a real device running this version, and the interpretation-layer contract is frozen against it.

> Fixtures under `tests/fixtures/` are sanitized reconstructions from public sources. Replacing them with sanitized HTML captured from a real device is recommended.

## Install

Grab a prebuilt binary (Linux / macOS × x86_64 / arm64; Linux builds are statically linked with musl) from [GitHub Releases](https://github.com/nogu3/ais/releases) and put it on your `PATH`:

```bash
tar xzf ais-v*-x86_64-unknown-linux-musl.tar.gz
install -m 755 ais-v*/ais ~/.local/bin/
```

Or build from source:

```bash
cargo install --path .
```

## Configuration

Credentials are passed via arguments or environment variables. No config file.

| Env var | Flag | Default | Description |
|---|---|---|---|
| `AISEG_HOST` | `--host` | (required) | AiSEG2 IP / hostname (e.g. `192.0.2.16`) |
| `AISEG_USER` | `--user` | `aiseg` | Username |
| `AISEG_PASS` | `--pass` | (required) | Password (prefer env to keep it out of shell history) |
| — | `--timeout` | `10` | HTTP timeout in seconds |

## Usage

```bash
export AISEG_HOST=192.0.2.16
export AISEG_PASS=********

# Instantaneous power (solar generation / grid purchase / total usage)
ais power
# {"generation_kw":0.5,"usage_kw":1.2,"buy_kw":0.7,"grid_direction":"buy","sources":[{"name":"太陽光","power_w":512}]}

# Distribution board (main + branch circuits, instantaneous, sorted by consumption)
ais circuits

# Daily energy totals (generation / usage / purchase / sale, kWh)
ais energy
ais energy --circuits          # include per-circuit kWh
ais energy --date 2026-06-09   # specific date

# Controllable devices (AiSEG2 is the source of truth)
ais devices

# Turn devices on / off (by name, id, or nodeId)
ais on "リビング照明"
ais off 1073741825:0x029101

# Read-only escape hatch: extract text of elements with id attributes from any page
ais fetch /page/graph/51111
```

## Exit codes

| Code | Meaning | stderr `kind` |
|---|---|---|
| 0 | Success | — |
| 2 | CLI argument error (clap) | — |
| 3 | Network / timeout | `network` / `timeout` |
| 4 | Authentication failed (401) | `auth_failed` |
| 5 | Unexpected HTTP status from AiSEG2 | `http_status` |
| 6 | Parse failure (selector mismatch = **possible firmware drift**) | `parse_failed` |
| 7 | Control rejected / result unconfirmed | `control_rejected` |
| 11 | Device not found in the control list / ambiguous match | `device_not_found` / `device_ambiguous` |

Errors are emitted to stderr as a single JSON line:

```json
{"error":{"kind":"parse_failed","detail":"no circuit entries found on electricflow/1113 (firmware mismatch?)"}}
```

## Architecture

```
src/
├── main.rs        # CLI (clap), command orchestration, exit codes
├── error.rs       # kind → exit code / stderr JSON
├── fetch/         # fetch layer: HTTP + hand-rolled Digest auth (ureq); never interprets content
│   └── digest.rs
├── parse/         # interpretation layer: all firmware-dependent fragile contracts live here
│   ├── power.rs     # POST /data/electricflow/111/update (JSON)
│   ├── circuits.rs  # GET /page/electricflow/1113?id=N (HTML)
│   ├── energy.rs    # GET /page/graph/5x111, 584, circuit catalog 734 (HTML)
│   ├── devices.rs   # device-control list traversal (HTML)
│   └── generic.rs   # generic extraction for `ais fetch`
└── control.rs     # interpretation layer: control payloads, change/check response parsing
```

If the firmware structure ever changes, only `parse/` and `control.rs` need fixing.

## Development

```bash
task build      # debug build
task test       # interpretation-layer tests against fixtures (no device needed)
task check      # fmt check + clippy + tests (run this before pushing)
task e2e        # E2E smoke against a mock AiSEG2 (incl. Digest auth, no device needed)
task run -- power
```

Plain cargo works too (`cargo build` / `cargo test` / `cargo clippy -- -D warnings`).

Releases: pushing a `v*` tag cross-builds binaries and publishes them (tar.gz + SHA256SUMS) to GitHub Releases. TLS is not linked since AiSEG2 speaks plain HTTP on the LAN only.

## License

MIT
