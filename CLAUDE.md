# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Power Ranger: passthrough power box. An ESP32-S3 polls an ABB B23 112-100 meter over Modbus RTU
(RS485) and serves a dashboard on its own WiFi AP (`PowerRanger`, captive portal,
`http://192.168.71.1`, the esp-idf-svc AP default). User-facing overview: README.md.

## Layout

- `core/` (crate `pr_core`): host-testable logic, no esp deps. All register knowledge (`b23`),
  the energy record (`record`: P·dt integration, 240-bucket graph that halves when full, NVS
  byte format with a version byte) and the captive DNS responder (`dns`).
- `fw/` - xtensa firmware. `main.rs`: boot-time record promotion, AP, DNS thread, HTTP routes,
  250 ms poll loop. `meter.rs`: RS485 master (+ `sim` feature with fake registers).
  `web/index.html`: the whole dashboard, one file, embedded with `include_bytes!`, fonts served
  from `/f/*.woff2`.
- `docs/registers.md` - register table, links the ABB manual. The PDF itself is gitignored:
  don't commit it, ABB's copyright.

Two separate crates on purpose, not a workspace: `fw/` pins the `esp` toolchain and an xtensa
target, which would break host `cargo test` for `core/`.

## Commands

```sh
cd core && cargo test                    # host, stable toolchain

. ~/export-esp.sh                        # espup env, once per shell
cd fw && PR_AP_PASS=<8..63 chars> cargo run --release --features sim   # flash + monitor, fake meter
cd fw && PR_AP_PASS=<8..63 chars> cargo run --release                  # real meter
```

`PR_AP_PASS` is the WPA2 password, compiled in via `env!` and checked at compile time; never commit it.
The runner flashes with `fw/partitions.csv` (1 MB NVS). Board: `/dev/cu.usbmodem*`
(native USB-Serial/JTAG, also the log console). Serial log prints the loaded last record at boot.

## Behavior that spans files

- Power-on = one record. `cur` is saved to NVS every 10 s; at boot `cur` becomes `last` only if
  `Record::counts()` (≥ 60 s and ≥ 1 Wh). Changing the byte format: bump `FORMAT` in
  `core/src/record.rs`; old blobs then load as `None` (record lost, no crash).
- Only fresh polls are integrated/graphed; after `MAX_MISSES` the link is down and readings are
  cleared, never shown stale. The page shows `NO METER` vs `NO BOX` (fetch failed).
- `/api` is polled at 4 Hz; graphs come from `/api/live` (5 min ring), `/api/rec`, `/api/last`.
  The page refetches all three when `uptime_s` goes backwards (box rebooted).
- Captive portal = DNS answering every A query with the AP IP + `/*` → 302 to `/`
  (`uri_match_wildcard`). Specific routes must be registered before `/*`.

## Hardware

- RS485: MAX485, TX GPIO17, RX GPIO18, DE/RE GPIO21. DE is held until `wait_tx_done`.
- Meter panel must match `fw/src/meter.rs`: 115200 8E1, slave 1.
- Rated current per phase for the load bars: `RATED_A` in `fw/web/index.html` (16 A plug).

## Conventions

- KISS, lean technical comments, only the why. Deliberate limits: `shortcut: <limit>, <when to upgrade>`.
- Register additions: extend `core/src/b23.rs` + a test, update `docs/registers.md`.
- Fonts: OFL. Share Tech Mono has Reserved Font Name "Share", so subsets must be renamed (done: "PR Mono").
- Branches `feat/…`, `fix/…`; no Claude co-author trailer on commits.
