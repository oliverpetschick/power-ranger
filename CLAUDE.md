# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Power Ranger: passthrough power box. An ESP32-S3 polls an ABB B23 112-100 meter over Modbus RTU
(RS485) and serves a live dashboard on its own WiFi AP (`PowerRanger`, `http://192.168.71.1`, the esp-idf-svc AP default).

## Layout

- `b23/` - host-testable lib, no esp deps: register addresses + decode into `Inst`/`Energy`.
  Invalid-value markers decode to `None`. All register knowledge lives here.
- `fw/` - xtensa firmware binary: AP, HTTP (`/` page, `/api` JSON), 1 s poll loop
  (`main.rs`); RS485 master (`meter.rs`); dashboard is one file `web/index.html` baked in
  with `include_str!`.
- `docs/registers.md` - register table used/unused, linking the ABB manual (ch. 9 = Modbus).
  The PDF itself is gitignored: don't commit it, ABB's copyright.

Two separate crates on purpose, not a workspace: `fw/` pins the `esp` toolchain and an xtensa
target, which would break host `cargo test` for `b23/`.

## Commands

```sh
cd b23 && cargo test                     # host, stable toolchain

. ~/export-esp.sh                        # espup env (LIBCLANG_PATH etc.), once per shell
cd fw && PR_AP_PASS=<8..63 chars> cargo run --release --features sim   # flash + monitor, fake meter
cd fw && PR_AP_PASS=<8..63 chars> cargo run --release                  # real meter
cd fw && PR_AP_PASS=... cargo build --release                          # build only
```

`PR_AP_PASS` is the WPA2 password, compiled in via `env!` and checked at compile time; never commit it.
First `fw` build downloads ESP-IDF v5.1.2 into `fw/.embuild/` (slow). Board enumerates as
`/dev/cu.usbmodem*` (native USB-Serial/JTAG, also the log console).

## Hardware

- RS485: MAX485, TX GPIO17, RX GPIO18, DE/RE GPIO21. DE is held until `wait_tx_done`.
- Meter serial settings must match `fw/src/meter.rs` consts: 19200 8E1, slave 1
  (set on the B23 panel, RS485 → Modbus).
- Box may be wired 1- or 3-phase; a phase is `active` when U ≥ 50 V (`b23::PHASE_MIN_V`).
- After 3 missed polls `link_ok` goes false and readings are cleared, never shown stale.

## Conventions

- KISS, lean technical comments, only the why.
- Deliberate limits are marked `shortcut: <limit>, <when to upgrade>`.
- Register additions: extend `b23` + a test, update `docs/registers.md`.
