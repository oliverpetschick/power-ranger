# ABB B23 Modbus registers

Source: ABB B23/B24 User Manual [2CMC485003M0201](https://library.abb.com/d/2CMC485003M0201) rev E, ch. 9 (p. 79–92).
Not redistributed here; keep a local copy in `docs/` (gitignored).

- FC 03 read, FC 06/16 write. 1–125 regs per read, readable range `0x1000–0x8EFF`, outside → exception 2.
- Addresses are on-wire (no 4xxxx offset, no −1).
- Multi-register: big-endian, high word first.
- Invalid/unsupported: unsigned → all `FFFF`, signed → `7FFF…`.
- Serial (set on meter panel, RS485 → Modbus): parity none/odd/even, 1200–… baud, address 1–247.

## Used

| Quantity | Reg | Size | Res. | Type |
|---|---|---|---|---|
| Active import | `5000` | 4 | 0.01 kWh | u64 |
| Active export | `5004` | 4 | 0.01 kWh | u64 |
| Voltage L1-N / L2-N / L3-N | `5B00` / `5B02` / `5B04` | 2 | 0.1 V | u32 |
| Current L1 / L2 / L3 | `5B0C` / `5B0E` / `5B10` | 2 | 0.01 A | u32 |
| Active power tot / L1 / L2 / L3 | `5B14` / `5B16` / `5B18` / `5B1A` | 2 | 0.01 W | i32 |
| Reactive power tot / L1–3 | `5B1C` … `5B22` | 2 | 0.01 var | i32 |
| Apparent power tot / L1–3 | `5B24` … `5B2A` | 2 | 0.01 VA | i32 |
| Frequency | `5B2C` | 1 | 0.01 Hz | u16 |
| Power factor tot / L1–3 | `5B3A` … `5B3D` | 1 | 0.001 | i16 |

## Not used yet

| Quantity | Reg | Size | Res. | Type |
|---|---|---|---|---|
| Active net | `5008` | 4 | 0.01 kWh | i64 |
| Reactive import / export / net | `500C` / `5010` / `5014` | 4 | 0.01 kvarh | u64 / u64 / i64 |
| Apparent import / export / net | `5018` / `501C` / `5020` | 4 | 0.01 kVAh | u64 / u64 / i64 |
| Active import tariff 1–4 | `5170` … `517C` | 4 | 0.01 kWh | u64 |
| Active export tariff 1–4 | `5190` … `519C` | 4 | 0.01 kWh | u64 |
| Active import / export / net L1–3 | `5460` … `5480` | 4 | 0.01 kWh | u64 / u64 / i64 |
| Resettable active import / export | `552C` / `5530` | 4 | 0.01 kWh | u64 |
| Voltage L1-L2 / L3-L2 / L1-L3 | `5B06` / `5B08` / `5B0A` | 2 | 0.1 V | u32 |
| Current N | `5B12` | 2 | 0.01 A | u32 |
| Phase angle power tot / L1–3 | `5B2D` … `5B30` | 1 | 0.1° | i16 |
| Phase angle voltage L1–3 | `5B31` … `5B33` | 1 | 0.1° | i16 |
| Phase angle current L1–3 | `5B37` … `5B39` | 1 | 0.1° | i16 |
| Quadrant tot / L1–3 | `5B3E` … `5B41` | 1 | 1–4 | u16 |
| Output 1 / 2 (R/W) | `6300` / `6301` | 1 | 0/1 | u16 |
| Serial number | `8900` | 2 | | u32 |
| FW version | `8908` | 8 | | ASCII |
| Type designation | `8960` | 6 | | ASCII |
| Current tariff (R/W) | `8A07` | 1 | 1–4 | u16 |
| Power fail counter | `8A2F` | 1 | | u16 |
| Reset resettable import / export (W, value 1) | `8F1B` / `8F1C` | 1 | | u16 |
