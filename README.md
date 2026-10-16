# Power Ranger

A passthrough power box with an energy meter. An **ABB B23 112-100** sits in the line and an **ESP32-S3** reads it over Modbus RTU (RS485). The ESP opens its own WiFi. Join it with a phone or laptop and the dashboard opens by itself (captive portal).

- **Live:** total power to 0.01 W, plus V / A / W / PF per phase and current against the 16 A rating. A 5-minute graph has one line per phase. Values update 4× per second.
- **Record:** power-on starts a run, power loss ends it. The page shows time, kWh, average and peak power, and a graph of the whole run. After the next power-on, the run is shown as **LAST RECORD**. Runs under 60 s or 1 Wh don't replace it, so plugging in just to read the last record keeps it.

Works on 1- or 3-phase wiring; phases below 50 V are hidden.

## Hardware

| Part | Notes |
|---|---|
| ABB B23 112-100 | 3-phase, direct connected, 65 A, RS485. Panel: RS485 → Modbus, **115200 baud, even parity, address 1** |
| ESP32-S3 | dev board with native USB (USB-Serial/JTAG), 16 MB flash |
| MAX485 module | TX → GPIO17, RX → GPIO18, DE+RE → GPIO21; A/B to the meter's RS485 terminals |
| Plug / socket | rated 16 A per phase (`RATED_A` in `fw/web/index.html`) |

Register map: [docs/registers.md](docs/registers.md).

## Build and flash

Needs the Rust ESP toolchain (`espup install --targets esp32s3`) and `espflash` (`cargo install espflash`).

```sh
. ~/export-esp.sh
cd fw
PR_AP_PASS='your-wifi-password' cargo run --release --features sim   # fake meter data
PR_AP_PASS='your-wifi-password' cargo run --release                  # real meter
```

- `PR_AP_PASS` is the WiFi password (WPA2, 8–63 characters). It's compiled into the firmware and never stored in the repo.
- The first build downloads ESP-IDF (slow).
- `cargo run` flashes with `fw/partitions.csv` and opens the serial monitor.
- On a board flashed before the 1 MB NVS partition existed, add `--erase-parts nvs` once:
  `espflash flash --partition-table partitions.csv --erase-parts nvs target/xtensa-esp32s3-espidf/release/power_ranger`

Tests (host, no hardware):

```sh
cd core && cargo test
```

## Use

1. Plug the box in and connect the device under test.
2. Join the WiFi **PowerRanger** and the dashboard opens. If you closed it, open **`http://power.ranger`**. Type the `http://`, otherwise the browser may search instead. Any name works, since the box answers all DNS lookups; `http://192.168.71.1` works too.
3. **LIVE** shows what the device draws right now; **RECORD** shows the run since power-on.
4. Unplug when done. At the next power-on, the run is under **LAST RECORD**.

## Layout

- `core/`: register decoding, the record (integration, graph buckets, NVS format) and the captive DNS. Plain Rust, tested on the host.
- `fw/`: the firmware. It runs the AP, the 250 ms meter poll loop, NVS saves every 10 s, and the HTTP API and page.
  - `fw/web/`: dashboard page and fonts, embedded into the firmware.
- `docs/`: register table. The ABB manual itself is linked there, not committed (ABB copyright).

API: `GET /api` (live values and record summaries), `/api/live` (last 5 min), `/api/rec`, `/api/last` (graphs).

## Licenses

Fonts in `fw/web/` are under the SIL Open Font License 1.1, see [fw/web/OFL.txt](fw/web/OFL.txt): a renamed subset of Share Tech Mono, and a subset of Saira.
