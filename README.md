# renogymon

Tools for monitoring Renogy BMS batteries via Bluetooth and serial, with APRS telemetry and a terminal UI.

## Binaries

- **renogymon-bms-collector** -- Collects BMS data over Bluetooth and exports metrics to VictoriaMetrics
- **renogymon-aprs** -- Beacons battery telemetry over APRS, via a TNC (Direwolf AGW), APRS-IS, or both
- **renogymon-tui** -- Terminal UI for live battery monitoring
- **serial-query** -- Query BMS over serial/Modbus
- **bt2-query** -- Query BMS over Bluetooth

## Installing

### From .deb package

Download the appropriate .deb from the [releases page](https://github.com/charlieh0tel/renogymon/releases) and install:

```bash
sudo dpkg -i renogymon-*_*.deb
```

### From source

Requires Rust 1.89+ (see `rust-toolchain.toml`).

```bash
cargo install --path .
```

## Systemd Services

The repo includes systemd unit files for running `renogymon-bms-collector` and `renogymon-aprs` as system services. When installed from a .deb, the service files are placed in `/usr/lib/systemd/system/`.

### Configuration

Edit the per-service files under `/etc/default/` (e.g. `renogymon-collector`, `renogymon-aprs`), installed by the .deb:

```bash
sudo editor /etc/default/renogymon-aprs
```

- **SSID** -- APRS SSID, i.e. callsign-N (e.g. `Y0URS-12`). Defaults to `N0CALL`, which `renogymon-aprs` will reject at startup.
- **COLLECTOR_ARGS** -- Arguments for `renogymon-bms-collector`. Defaults to `bt2`. Examples: `bt2 --adapter hci1`, `serial --port /dev/ttyUSB0`.

`renogymon-aprs` also reads these optional environment variables (see `/etc/default/renogymon-aprs`):

- **APRS_TACTICAL** -- Optional tactical source callsign (e.g. `SOLAR1`). When set, beacons are sourced from it and the operator's base callsign (**SSID** without the SSID suffix) is appended to each telemetry packet as an identifying comment. **SSID** still drives the APRS-IS login and passcode.
- **APRS_LATITUDE** / **APRS_LONGITUDE** -- Static station position in decimal degrees. Read once at startup. aprs.fi only collects telemetry for stations that have sent a position, so set one to be mapped and have telemetry shown.
- **APRS_GPSD** -- Alternatively, read the position once at startup from gpsd at `HOST[:PORT]` (e.g. `localhost:2947`). Static coordinates take precedence if both are set. If gpsd is configured but no fix is obtained, the service exits and systemd restarts it to retry.
- **APRS_GPSD_FIX_TIMEOUT** -- Seconds to wait for a gpsd fix at startup before exiting to retry (default 30). Raise it if the GPS is slow to lock from cold.
- **APRS_SYMBOL** / **APRS_POSITION_COMMENT** -- Optional APRS symbol (table selector + code, default `/-`) and comment for the position beacon.
- **APRS_TRANSPORT** -- `agw` (TNC, default), `aprs-is` (internet), or `both`.
- **APRSIS_HOST** / **APRSIS_PORT** -- APRS-IS server (default `rotate.aprs2.net:14580`). The passcode is computed from the callsign automatically.

### Enabling the Services

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now renogymon-bms-collector
sudo systemctl enable --now renogymon-aprs
```

### Managing the Services

```bash
systemctl status renogymon-aprs
systemctl status renogymon-bms-collector

journalctl -u renogymon-aprs -f
journalctl -u renogymon-bms-collector -f
```

## Grafana

See [GRAFANA.md](GRAFANA.md) for metric names, derived power/energy metrics,
and panel queries.

## License

MIT
