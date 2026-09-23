# APRS SMS Alerts for BMS Alarms

Note: WIP

## Overview

`renogymon-aprs` will send SMS alerts via SMSGTE — an APRS-IS gateway that converts APRS messages to SMS — when the BMS raises a fault. Supports onset, cleared, and periodic repeat messages to one or more phone numbers.

---

## Changes

| File | Action |
|---|---|
| `aprs/Cargo.toml` | Add `chrono` (with `serde`), `serde` (with `derive`), `serde_json` |
| `aprs/src/sink.rs` | Add `Packet::Message(String)` variant |
| `aprs/src/sms.rs` | New: SMSGTE formatting + `AlarmNotifier` |
| `aprs/src/lib.rs` | Add `pub mod sms;` |
| `aprs/src/bin/renogymon-aprs.rs` | Add CLI args, refactor loop, wire notifier |

---

## Step 1 — `sink.rs`: add `Packet::Message`

Add a fourth variant carrying a single APRS info field (the `:SMSGTE...:...` string).

```rust
pub enum Packet {
    Position(String),
    Telemetry(String),
    Definitions(Vec<String>),
    Message(String),   // single APRS info field (e.g. an SMSGTE message)
}
```

Update `payloads()` and `kind()`:
```rust
Packet::Position(f) | Packet::Telemetry(f) | Packet::Message(f) => std::slice::from_ref(f),
// ...
Packet::Message(_) => "message",
```

No other sink changes needed — both `AgwTransmitter` and `AprsIsTransmitter` already handle arbitrary info fields.

---

## Step 2 — `aprs/src/sms.rs`: formatting and `AlarmNotifier`

### Constants and formatting

Alarms have two priority tiers, each with its own configurable repeat interval:
- **Fault**: serious conditions (OV, UV, OC, OT, UT, SC) — default repeat 1 hour
- **Status**: informational (HEATER_ON) — default repeat 24 hours

`FULLY_CHARGED` is excluded (not actionable).

```rust
#[derive(Copy, Clone)]
enum AlarmPriority { Fault, Status }

const ALARM_BITS: [(SystemAlarms, &str, AlarmPriority); 7] = [
    (SystemAlarms::OVER_VOLTAGE,  "OV",  AlarmPriority::Fault),
    (SystemAlarms::UNDER_VOLTAGE, "UV",  AlarmPriority::Fault),
    (SystemAlarms::OVER_CURRENT,  "OC",  AlarmPriority::Fault),
    (SystemAlarms::OVER_TEMP,     "OT",  AlarmPriority::Fault),
    (SystemAlarms::UNDER_TEMP,    "UT",  AlarmPriority::Fault),
    (SystemAlarms::SHORT_CIRCUIT, "SC",  AlarmPriority::Fault),
    (SystemAlarms::HEATER_ON,     "HTR", AlarmPriority::Status),
];

/// Format an APRS info field targeting SMSGTE for `phone` (E.164).
pub fn format_smsgte_payload(phone: &str, text: &str) -> String {
    format!(":SMSGTE   :@{phone} {text}")
}

fn describe_alarms(alarms: SystemAlarms) -> String {
    ALARM_BITS.iter()
        .filter(|(bit, _, _)| alarms.contains(*bit))
        .map(|(_, label, _)| *label)
        .collect::<Vec<_>>()
        .join(" ")
}
```

### `AlarmNotifier`

Tracks per-bit state using fixed-size arrays indexed by `ALARM_BITS` position (same 6-element order). Uses `DateTime<Utc>` (not `Instant`) so state is directly serializable for crash recovery. Returns APRS info field strings; caller wraps them in `Packet::Message`.

```rust
/// Persisted subset of AlarmNotifier state. Serialized to/from the state file.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct AlarmState {
    /// Keyed by alarm label ("OV", "UV", ...).
    alarms: std::collections::HashMap<String, AlarmEntry>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AlarmEntry {
    onset_at: DateTime<Utc>,
    last_notified: DateTime<Utc>,
}

pub struct AlarmNotifier {
    phones: Vec<String>,
    repeat_interval: Duration,
    state_path: Option<PathBuf>,
    /// When each alarm bit first fired (None = currently inactive).
    onset_at: [Option<DateTime<Utc>>; 6],
    /// When we last sent an SMS for each alarm bit.
    last_notified: [Option<DateTime<Utc>>; 6],
}
```

#### `AlarmNotifier::new`

Loads `AlarmState` from `state_path` if it exists; populates `onset_at`/`last_notified` from the file. Alarms present in the state file are treated as already-active — no re-onset SMS on startup.

```rust
pub fn new(
    phones: Vec<String>,
    repeat_interval: Duration,
    state_path: Option<PathBuf>,
) -> Self
```

#### State persistence

After every `check()` call that modifies state, write the current `AlarmState` atomically:

```rust
fn save_state(&self) {
    let Some(path) = &self.state_path else { return };
    let tmp = path.with_extension("tmp");
    // serialize to tmp, then fs::rename(tmp, path)
}
```

`rename` is atomic on the same filesystem, so a crash mid-write leaves the old state intact.

#### `AlarmNotifier::check(current: SystemAlarms) -> Vec<String>`

State machine per bit:
- `(inactive, active=true)` → **onset**: record `onset_at[i] = now`, `last_notified[i] = now`, add to onset set
- `(active, active=true)` → **repeat check**: if `now - last_notified[i] >= repeat_interval`, update `last_notified[i]`, add to repeat set
- `(active, active=false)` → **cleared**: clear both slots, add to cleared set
- `(inactive, active=false)` → no-op

After iterating all bits, call `save_state()` if any state changed, then build SMS info fields:
- If onset non-empty: `format!("BMS ALARM: {}", describe_alarms(onset))` → one payload per phone
- If repeat non-empty: format each bit as `"LABEL(Xm)"` where X is `(now - onset_at[i]).num_minutes()` → one payload per phone
  - e.g. `"BMS ALARM: OV(30m) OC(10m)"` — each label carries its own alarm age
- If cleared non-empty: `format!("BMS CLEAR: {}", describe_alarms(cleared))` → one payload per phone

Returns all generated payloads in onset → repeat → cleared order.

---

## Step 3 — `lib.rs`

```rust
pub mod sms;
```

---

## Step 4 — `renogymon-aprs.rs`: CLI args and loop

### New `Args` fields

```rust
/// Phone numbers for SMS alerts (E.164, e.g. +15551234567). Comma-separated or repeated.
#[arg(long = "sms-to", env = "APRS_SMS_TO", value_delimiter = ',')]
sms_to: Vec<String>,

/// Minimum seconds between repeat SMS for a persisting alarm (default: 1 hour).
#[arg(long, default_value_t = 3600, env = "APRS_SMS_REPEAT_INTERVAL")]
sms_repeat_interval: u64,

/// Path to persist alarm state across restarts (prevents spurious re-onset SMS).
#[arg(long, env = "APRS_SMS_STATE_FILE")]
sms_state_file: Option<PathBuf>,
```

If `--sms-to` is set but `--sms-state-file` is not, log a warning that crash recovery is disabled.

### Refactor `build_beacon_packet` → `get_system_summary`

Refactor to return `SystemSummary` so alarms are accessible at the call site.

```rust
async fn get_system_summary(vm_client: &VmClient) -> Result<SystemSummary, String> {
    let batteries = vm_client.query_all_batteries().await.map_err(|e| e.to_string())?;
    if batteries.is_empty() {
        return Err("No batteries found".to_string());
    }
    Ok(SystemSummary::new(&batteries))
}
```

`format_telemetry_packet` stays the same and is called at the call site.

### `main` setup

```rust
let mut notifier = (!args.sms_to.is_empty()).then(|| {
    AlarmNotifier::new(
        args.sms_to.clone(),
        Duration::from_secs(args.sms_repeat_interval),
        args.sms_state_file.clone(),
    )
});
```

### Loop body (replaces `build_beacon_packet` call)

```rust
match get_system_summary(&vm_client).await {
    Ok(summary) => {
        queue(&sender, Packet::Telemetry(format_telemetry_packet(&summary, operator)));
        if let Some(ref mut notifier) = notifier {
            for payload in notifier.check(summary.alarms()) {
                queue(&sender, Packet::Message(payload));
            }
        }
    }
    Err(e) => error!(error = %e, "Failed to build beacon"),
}
```

---

## Tests (in `sms.rs`)

- `describe_alarms` returns correct labels for known bit patterns
- `format_smsgte_payload` produces correct `:SMSGTE   :@+1...` info field
- `AlarmNotifier::check` state machine:
  - First call with active alarm → onset payload returned, state recorded
  - Second call within repeat interval → no payload
  - Third call past repeat interval → repeat payload with per-alarm age
  - Alarm cleared → cleared payload, state reset
  - Re-onset after clear → onset payload again
- Crash recovery: construct `AlarmNotifier` with pre-populated state from file; call `check()` with current alarms; alarms already in state file → no re-onset, respects `last_notified` for repeat interval

---

## Step 5 — `aprs/src/bin/renogymon-sms-test.rs`: test CLI

Standalone binary that sends a single test SMS and exits. Reuses `SinkConfig`, `spawn_receivers`, and `format_smsgte_payload` from the library.

### Args

Same APRS connection args as `renogymon-aprs` (ssid, transport, agw-host/port, aprsis-host/port, tocall) plus:

```rust
/// Phone number(s) to send the test SMS to (E.164). Repeatable.
#[arg(long = "sms-to", required = true)]
sms_to: Vec<String>,

/// Message body to send (default: "BMS SMS TEST").
#[arg(long, default_value = "BMS SMS TEST")]
message: String,
```

### Behavior

Build `SinkConfig`, call `spawn_receivers`, send one `Packet::Message` per phone number via `format_smsgte_payload`, drop sender, await handles, exit. No loop, no VictoriaMetrics dependency.

---

## Verification

```sh
cargo test
cargo clippy
cargo +nightly fmt
```

Manual smoke test (logs should show `kind=message` sent):
```sh
renogymon-aprs --ssid N0CALL-12 --sms-to +15551234567 \
  --transport aprs-is --once --vm-url http://localhost:8428
```

Check `tracing` output for `transport=aprs-is kind=message Sent` lines.
