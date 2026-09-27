# renogymon-archiver Design Document

## Goal

Export historical BMS metrics from a local VictoriaMetrics instance to Parquet files
and stage them locally on the RPi4. An always-on **archive host** on the same
Tailscale network periodically *pulls* the staged files over rsync/SSH and removes
them from the RPi4 on success.

This keeps the RPi4's VM instance bounded to a reasonable rolling retention window
(e.g. 12 months) while preserving all data permanently on a remote Ubuntu host as a
Parquet corpus for long-term offline analysis (pandas / pyarrow / DuckDB / Jupyter /
Colab).

Live and recent dashboards stay in Grafana Cloud within its own retention limits;
the Parquet archive is the permanent, self-owned record for ad-hoc analysis — it is
not wired into Grafana.

## Constraints

- Rust binary, packaged into the existing `.deb` via `cargo-deb`
- **No private key on the RPi4.** Transfer is pull-based: the archive host initiates
  the rsync and holds the private key. The Pi holds only the puller's *public* key in
  a restricted `authorized_keys`. A compromised/stolen Pi exposes no credential.
- Resilient to intermittent Tailscale connectivity — local staging accumulates until
  the next successful pull
- Decoupled: the Pi only **exports**; pulling and deleting are the archive host's job
- Fits existing project conventions: tokio async, clap derive, tracing, chrono, serde, reqwest+rustls

## Architecture

```
RPi4
  VictoriaMetrics (localhost:8428, ~12mo retention)
      |  HTTP /api/v1/export
      v
  renogymon-archiver export            (systemd timer, daily)
      |
      v
  /var/lib/renogymon-archiver/staging/renogy_YYYY-MM-DD.parquet   (one file per day)
      ^
      |  rsync --remove-source-files over SSH, INITIATED BY the archive host
      |  (Tailscale). On success the source files are deleted from the RPi4.
Archive host  (always-on, on Tailscale; holds the SSH private key)
  rsync pull            (cron/systemd timer)
      |
      v
  ~/renogy-archive/renogy_YYYY-MM-DD.parquet
```

## State File

Path: `/var/lib/renogymon-archiver/state.json`

```json
{
  "last_exported_day": "2025-11-29"
}
```

- `last_exported_day` is the most recent calendar day (UTC) fully exported to Parquet
- On first run (no state file), export starts from the earliest day available in VM
  (auto-detected), or from `--start-date` if given
- Export always stops at `today - 1` (never export the current partial day)
- A day is committed to state **only after** its Parquet file is written + fsynced
  **and** the VM query for that day returned successfully. A failed/incomplete VM
  query aborts that day and leaves state untouched, so the day is retried next run —
  the archiver never advances past a day it could not fully read.
- An empty day (VM returned no samples) is valid: it commits to state with no file
  written. Empty ≠ error — only a query *failure* blocks advancing.
- **Readiness guard:** a successful-but-empty `/api/v1/export` is indistinguishable
  from a real empty day, so a VM that is up but not yet serving (mid-restart/replay)
  could otherwise advance state past real days permanently. Before the day loop, if
  state shows prior progress but VM reports **no renogy series at all**, the run aborts
  without advancing and retries next run. (Residual: a *partial* replay that serves
  some series but not the target days can still advance past them — run export only
  once VM is fully ready, e.g. don't trigger it immediately after a VM restart.)

## VM Export API

Use VictoriaMetrics `/api/v1/export` endpoint (JSON lines format):

```
GET http://localhost:8428/api/v1/export
  ?match[]={__name__=~"renogy_.*"}
  &start=<unix_timestamp>
  &end=<unix_timestamp>
```

Restrict the match to `renogy_.*` so we only archive our own series, not
anything else the VM instance might be scraping.

Response is newline-delimited JSON, one object per time series:

```json
{"metric":{"__name__":"renogy_soc_percent_value","job":"..."},"values":[1.0,2.0],"timestamps":[1700000000000,1700000001000]}
```

Timestamps are milliseconds since epoch.

Export one day at a time (midnight to midnight UTC) to produce one Parquet file per day.

## Parquet Schema

Long/narrow format — one row per sample:

| Column      | Type                  | Notes                        |
|-------------|-----------------------|------------------------------|
| `timestamp` | `TIMESTAMP(MILLIS, UTC)` | Parquet logical timestamp (physically INT64 ms). Reads as `datetime64[ns, UTC]` in pandas and `TIMESTAMP` in DuckDB with no conversion |
| `metric`    | `UTF8`                | e.g. `renogy_soc_percent_value` |
| `value`     | `DOUBLE`              |                              |
| `labels`    | `UTF8`                | JSON-encoded extra labels, omitting `__name__`, `job`, and `instance`; preserves `battery`, `cell`, `sensor`, etc. Empty string if none |

Compression: Snappy (default parquet-rs compression, good ratio/speed tradeoff).

Row group size: default (128MB) is fine; files will be much smaller than this.

## CLI

The Pi-side binary only exports. Transfer is the archive host's job (plain rsync,
not this binary — see "Pull"), so there is no `transfer` subcommand and no
SSH/remote options.

```
renogymon-archiver [OPTIONS] <COMMAND>

Commands:
  export    Export unarchived days from VM to local Parquet staging dir
  status    Show last exported day and staged files

Options:
  --vm-addr <URL>           VictoriaMetrics base URL [default: http://localhost:8428]
  --staging-dir <PATH>      Local staging directory [default: /var/lib/renogymon-archiver/staging]
  --state-file <PATH>       State file path [default: /var/lib/renogymon-archiver/state.json]
  --start-date <YYYY-MM-DD> First-run backfill lower bound (overrides auto-detect of
                            the earliest day in VM). Ignored once state.json exists.
  --max-days <N>            Export at most N days this run, then stop (state advances
                            for the days done). Lets a large initial backfill run in
                            bounded chunks so staging never exceeds ~N days of files.
                            [default: unlimited]
  -v, --verbose             Enable verbose logging
```

`--start-date` is a safety valve: if auto-detecting the earliest day is uncertain, set
it to a date you know predates all data (e.g. the BMS install date). Days before any
data simply produce no rows and advance state harmlessly.

The `export` subcommand:
- Reads state file to find `last_exported_day`
- Exports each day from `last_exported_day + 1` through `today - 1` inclusive
- Writes each day to a temp file (e.g. `staging/.renogy_YYYY-MM-DD.parquet.tmp`),
  fsyncs, then **atomically renames** into `staging/renogy_YYYY-MM-DD.parquet`. So
  the staging dir only ever contains *complete* files — a crash or `ENOSPC` mid-write
  leaves at most a discarded temp, never a partial file the puller could pull
- Updates state file after each successfully renamed file
- Skips days where a Parquet file already exists in staging dir (idempotent)
- Honors `--max-days N`: exports the N oldest unexported days then stops (state
  advances for those done), so a large backfill can be drained in bounded chunks

Export must **never delete** staged files — deletion is driven solely by the puller
on the archive host (see below). Export's idempotency comes from the state file
(`last_exported_day`), not from files lingering in staging, so a day already pulled
and deleted is not re-exported.

The `status` subcommand (Pi side) reports export *progress*, not archive
completeness (the Pi can't see pulled-and-deleted files):
- earliest exported day and `last_exported_day`
- number of days covered by the exported range
- staged-but-not-yet-pulled files (count + date range) — i.e. the current backlog
- because export advances strictly day-by-day and only on a successful read, the
  exported range is contiguous by construction; full-archive gap auditing happens on
  the archive host (see puller `status`).

## Pull (runs on the archive host)

The archive host pulls staged files and removes them from the Pi on success. This is
done by `renogymon-archiver-puller`, a small Rust binary (separate workspace crate, see
"Workspace & Packaging") that reads config and **shells out to the system `rsync`**:

```
rsync -a --remove-source-files --partial \
  -e "ssh -i <ARCHIVER_SSH_KEY> -o BatchMode=yes" \
  <ARCHIVER_REMOTE>            # e.g. renogymon-archiver@rpi4:./
  <ARCHIVER_DEST>/             # e.g. /var/lib/renogy-archive/
```

- `--remove-source-files` deletes each file *on the Pi* only after that file is
  confirmed transferred. A mid-batch failure (Tailscale drops) leaves the not-yet-
  pulled files on the Pi for the next run — never an all-or-nothing delete.
- Only `*.parquet` are pulled; `state.json` stays on the Pi (rrsync is scoped to the
  staging dir, which contains only Parquet files — keep `state.json` in the parent
  `/var/lib/renogymon-archiver`, not in `staging/`).
- The binary adds a `flock` guard (timer + manual runs can't overlap) and structured
  `tracing` logging on top of the rsync invocation.
- Driven by a systemd timer on the archive host (see Systemd Units).

## Backlog Model

The unit of work is **one complete, immutable Parquet file per UTC day**, written
once and never mutated. Export and pull are fully decoupled:

- `export` keeps minting daily files regardless of connectivity (one file per day,
  catching up every unexported day since `last_exported_day`).
- the archive host's pull drains whatever has accumulated whenever the link is up.

There is no byte-level delta of the data — rsync only decides *which whole files* are
missing on the archive host. The "backlog" is simply the set of daily files still on
the Pi; during a Tailscale outage they pile up in staging and the next successful
pull ships them all and clears them.

## New Dependencies (Cargo.toml)

```toml
arrow = { version = "58", default-features = false }
parquet = { version = "58", default-features = false, features = ["arrow", "snap"] }
serde_json = "1"
# chrono gains the "serde" feature (NaiveDate in the state file)
```

Note: pin `arrow` and `parquet` to the same major version.

## Workspace & Packaging

`renogymon` is a Cargo workspace: the root is the shared **library** (plus dev
query/example bins, not packaged), and each shipped tool is its own member crate
producing its own `.deb` via `cargo-deb`. No `dpkg-deb`/`fpm`, no second repo.

```
renogymon/                  # root = library crate (renogy)
  src/                      # lib + dev bins (bt2-query, serial-query, example)
  collector/                # renogymon-bms-collector + renogymon-tui   -> deb renogymon-collector
  aprs/                     # renogymon-aprs                          -> deb renogymon-aprs
  archiver/                 # renogymon-archiver (self-contained)     -> deb renogymon-archiver
  puller/                   # renogymon-archiver-puller               -> deb renogymon-archiver-puller
```

- `collector`/`aprs` depend on the `renogy` lib (path); `archiver`/`puller` are
  self-contained (the archiver pulls `arrow`/`parquet` out of the lib entirely).
- Each member ships its own systemd unit(s), `sysusers.d` user, and (collector/aprs)
  `/etc/default/<pkg>` conf-file. Units use cargo-deb's `systemd-units`
  integration with `enable = true, start = true`: install enables each timer (or
  service) and starts it immediately.  Configure promptly after install.
- `archiver` depends on `rsync` (the Pi serves the rrsync pull) and recommends
  `openssh-server`.
- Collector BT-2 access relies on modern BlueZ's default D-Bus policy — no BlueZ
  dependency (see "Non-root Posture").

CI builds each with `cargo deb -p <package> --target <arch>` for both amd64 and arm64,
disambiguating artifacts via the `artifact-suffix` input.

## Systemd Units

### Pi side: fixed service user

A pull needs a stable login identity on the Pi that *owns* the staging files (the
puller reads and deletes them). `DynamicUser=yes` can't provide that — its ephemeral
UID owns the StateDirectory at mode 0700 and a separate SSH login user can't read or
unlink there. So export runs as a fixed, non-root system user provisioned by
`sysusers.d`.

`systemd/renogymon-archiver.sysusers` (installed to `/usr/lib/sysusers.d/renogymon-archiver.conf`):

```
#Type Name             ID  GECOS              Home directory            Shell
u     renogymon-archiver  -   "Renogy archiver"  /var/lib/renogymon-archiver  /bin/sh
```

The shell is `/bin/sh` (not `nologin`) because sshd execs the forced rrsync command
via the user's shell; the `command=` restriction in `authorized_keys` is what actually
confines the account. Key-only, no password.

sysusers never modifies an existing user, so the postinst also switches an existing
`renogymon-archiver` account from `nologin` to `/bin/sh`.

`tmpfiles/renogymon-archiver.conf` (installed to `/usr/lib/tmpfiles.d/`) creates the
home, `staging/`, `.ssh/` (0700) and an empty `.ssh/authorized_keys` (0600), all
owned by `renogymon-archiver`, so key setup is just adding one line.

### renogymon-archiver-export.service

```ini
[Unit]
Description=Export Renogy metrics to Parquet
After=victoria-metrics.service

[Service]
Type=oneshot
User=renogymon-archiver
Group=renogymon-archiver
StateDirectory=renogymon-archiver
ExecStart=/usr/bin/renogymon-archiver export
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
ProtectKernelTunables=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes
```

`StateDirectory=renogymon-archiver` creates `/var/lib/renogymon-archiver` owned by
`renogymon-archiver`. Staging goes in `staging/` under it; `state.json` sits in the
parent so the rrsync-scoped pull never touches it.

### renogymon-archiver-export.timer

```ini
[Unit]
Description=Run Renogy metrics export daily

[Timer]
OnCalendar=daily
Persistent=true

[Install]
WantedBy=timers.target
```

### Archive host side: pull units

Shipped by the separate puller package (below), not the Pi `.deb`. The service runs a
small wrapper script; failures are retried within the run and by the daily timer.

`renogymon-archiver-puller.service`:

```ini
[Unit]
Description=Pull Renogy Parquet archives from RPi4
After=network-online.target tailscaled.service
Wants=network-online.target

[Service]
Type=oneshot
User=renogymon-archiver-puller
Group=renogymon-archiver-puller
EnvironmentFile=/etc/default/renogymon-archiver-puller
ExecStart=/usr/bin/renogymon-archiver-puller pull
Restart=on-failure
RestartSec=300
StartLimitBurst=3
```

`renogymon-archiver-puller.timer`:

```ini
[Unit]
Description=Pull Renogy Parquet archives daily

[Timer]
OnCalendar=daily
Persistent=true

[Install]
WantedBy=timers.target
```

`Persistent=true` re-runs a *missed* fire at next boot. A fire that *runs but fails*
(Tailscale down) is retried by `Restart=on-failure` within the run, and otherwise the
next daily fire pulls the accumulated backlog.

## Archive Host Puller Package (`renogymon-archiver-puller`)

A second Rust crate in the workspace (`puller/`), built and packaged by `cargo-deb`
like the main crate. It installs:

```
/usr/bin/renogymon-archiver-puller                          # the Rust binary
/usr/lib/systemd/system/renogymon-archiver-puller.service     # started on install
/usr/lib/systemd/system/renogymon-archiver-puller.timer       # enabled on install
/usr/lib/sysusers.d/renogymon-archiver-puller.conf          # creates the puller user
/usr/lib/tmpfiles.d/renogymon-archiver-puller.conf          # creates state + dest dirs
/etc/default/renogymon-archiver-puller                      # conf-file (config)
```

### The binary

`renogymon-archiver-puller` parses config (clap, with env fallback from the
`EnvironmentFile`). Runtime deps: `rsync` + `openssh-client`. Two subcommands:

```
renogymon-archiver-puller <COMMAND>

Commands:
  pull      Pull staged files from the Pi and delete-on-success (run by the timer)
  status    Audit the local archive dir for completeness / gaps
```

- **`pull`** takes a `flock`, then execs the system `rsync` as shown under "Pull".
  Because the Pi's key is rrsync-scoped to the staging dir, `ARCHIVER_REMOTE`'s path is
  *relative to that locked dir* — use `./` to pull everything staged.
- **`status`** scans `ARCHIVER_DEST` for `renogy_YYYY-MM-DD.parquet`, parses the dates,
  and reports:
  - first day, last day, and total files present
  - **every missing calendar day** in `[first .. last]` (the gap list), with
    consecutive missing days collapsed into ranges
  - this is the cutover-verification command: an empty gap list over the expected
    range means the full history is safely on the archive host. (A missing day is
    flagged conservatively — it may be a genuinely data-less day, e.g. the system was
    down all day, but either way it's worth surfacing.)

### `puller/Cargo.toml` (sketch)

```toml
[package]
name = "renogymon-archiver-puller"
version = "0.1.0"
edition = "2024"

[dependencies]
clap = { version = "4", features = ["derive", "env"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[package.metadata.deb]
depends = "$auto, rsync, openssh-client"
assets = [
  ["target/release/renogymon-archiver-puller", "usr/bin/", "755"],
  ["systemd/renogymon-archiver-puller.service", "usr/lib/systemd/system/", "644"],
  ["systemd/renogymon-archiver-puller.timer", "usr/lib/systemd/system/", "644"],
  ["sysusers/renogymon-archiver-puller.conf", "usr/lib/sysusers.d/renogymon-archiver-puller.conf", "644"],
  ["tmpfiles/renogymon-archiver-puller.conf", "usr/lib/tmpfiles.d/renogymon-archiver-puller.conf", "644"],
  ["default/renogymon-archiver-puller", "etc/default/", "644"],
]
conf-files = ["/etc/default/renogymon-archiver-puller"]
```

The `systemd-units` integration enables the timer and starts the service on
install.  Until `ARCHIVER_REMOTE` is set and the key is installed on the Pi, that
first pull fails (and retries per `Restart=on-failure`); this is harmless.  The
service is a oneshot with no `[Install]` section, so enable the **timer**, not the
service; a manual `systemctl start` blocks until the pull (and any restart delay)
finishes.

Likewise the Pi's `renogymon-archiver` package enables the export timer and starts an
export on install.  If derived metrics need backfilling into VM, do that before the
first export, since exported days are never rewritten.

### Config — `/etc/default/renogymon-archiver-puller`

```sh
# Source: <pi-ssh-user>@<pi-tailscale-name>:<path-relative-to-rrsync-root>
ARCHIVER_REMOTE=renogymon-archiver@rpi4-tailscale-name:./

# Where to land the Parquet corpus on this host
ARCHIVER_DEST=/var/lib/renogy-archive

# Private key (lives only on this host)
ARCHIVER_SSH_KEY=/var/lib/renogymon-archiver-puller/id_ed25519
```

### sysusers / tmpfiles

`sysusers/renogymon-archiver-puller.conf`:
```
u renogymon-archiver-puller - "Renogy archive puller" /var/lib/renogymon-archiver-puller /usr/sbin/nologin
```
(`nologin` is fine here — this user never *receives* SSH; it only initiates it.)

`tmpfiles/renogymon-archiver-puller.conf`:
```
d /var/lib/renogymon-archiver-puller 0700 renogymon-archiver-puller renogymon-archiver-puller -
d /var/lib/renogy-archive         0755 renogymon-archiver-puller renogymon-archiver-puller -
```

The dest dir mode (`0755`) lets your analysis tooling (Jupyter as your own user) read
the corpus; tighten/loosen to taste, or point `ARCHIVER_DEST` under your home. systemd's
dpkg triggers run sysusers/tmpfiles on install (postinst fallback on older systemd).

## Non-root Posture (all services)

All services run non-root, each as its own dedicated user (least privilege; no shared
blast radius), provisioned from a shipped `sysusers.d` file in each member's deb:

```
u renogymon-archiver  - "Renogy archiver"
u renogymon-collector - "Renogy BMS collector"
u renogymon-aprs      - "Renogy APRS reporter"
```

| Service                | User               | Notes                                                |
|------------------------|--------------------|------------------------------------------------------|
| `renogymon-archiver`      | `renogymon-archiver`  | owns its StateDirectory                              |
| `renogymon-aprs`          | `renogymon-aprs`      | TCP client to direwolf AGW + HTTP to VM             |
| `renogymon-bms-collector` | `renogymon-collector` | `SupplementaryGroups=dialout` (serial); BT-2 via D-Bus |

- **`renogymon-aprs` — drop-in.** `User=`/`Group=renogymon-aprs`. Keep existing hardening;
  do **not** re-add `ProtectSystem=strict` (a prior commit dropped it for the direwolf
  loopback).
- **`renogymon-bms-collector` — non-root, no BlueZ coupling.** `User=renogymon-collector`,
  `SupplementaryGroups=dialout` (serial mode; `dialout` is base-system, not bluez).
  For BT-2 it talks to BlueZ over the system D-Bus, and on modern BlueZ the **default**
  D-Bus policy already permits any local user to `send_destination="org.bluez"` (group-
  based access was dropped; polkit only gates privileged ops like pairing, which the
  collector doesn't do). So no `bluetooth` group, no custom D-Bus policy, no `Depends:
  bluez`. **Verify on the Pi:** if a locked-down BlueZ build returns D-Bus
  `AccessDenied`, the minimal fallback is a self-contained policy granting
  `renogymon-collector` access to `org.bluez` (still no package dep / no group).

## Configuration

The **Pi needs no config file** — `renogymon-archiver export` uses defaults (or flags in
the unit). All transfer configuration lives on the **archive host** (see the puller
package below).

## SSH Key Setup (One-Time)

Direction is inverted from a push: the **private** key lives on the archive host; the
Pi holds only the matching **public** key, restricted to rrsync on the staging dir.

1. **Generate the keypair on the archive host:**

```sh
ssh-keygen -t ed25519 -f /var/lib/renogymon-archiver-puller/id_ed25519 -N "" \
  -C "renogymon-archiver-puller"
```

2. **Install the public key on the Pi** by adding one line to
   `/var/lib/renogymon-archiver/.ssh/authorized_keys` (the package creates the file),
   locked to rrsync scoped to the staging dir.  `from=` optionally limits the key to
   the archive host's (Tailscale) address:

```
from="100.x.y.z",command="rrsync /var/lib/renogymon-archiver/staging",no-pty,no-agent-forwarding,no-port-forwarding,no-X11-forwarding ssh-ed25519 AAAA... renogymon-archiver-puller
```

`rrsync` (ships with rsync) confines the connection to that one directory and rejects
anything but an rsync transfer. Read-write is required (not `-ro`) so the puller's
`--remove-source-files` can delete the staged files after a successful pull.

The Pi never holds a private key. If it's lost or stolen, no credential leaks.

## VM Retention & First-Run Cutover (data-loss safety)

The archiver only **reads** from VM (`/api/v1/export`); it never deletes or mutates
VM data. The single data-loss vector is VM retention expiring data before it is
archived — so reduce retention **last**, only after the full history is archived and
verified.

VM's default `-retentionPeriod` is **1 month**, not unlimited.  If it was never set,
data older than ~30 days is already gone and more expires daily until export runs.

**Cutover order — never lower retention first:**

1. Check VM's `-retentionPeriod` (e.g. `systemctl cat victoria-metrics`).  If it is
   unset or shorter than the history you want to keep, raise it now (e.g.
   `-retentionPeriod=100y`); otherwise leave it as is.
2. Run `renogymon-archiver export` — backfills the entire VM history into staging
   (one Parquet file per day, earliest day → yesterday).
3. Stand up the puller; pull everything to the archive host.
4. **Verify** the archive host holds a complete, contiguous set of daily files
   spanning VM's full range — check the first/last dates and spot-check a few days'
   row counts against the same range queried from VM.
5. **Only now** set `-retentionPeriod=12` (months) on VM (in its
   `ExecStart`/override). VM drops data older than 12 months on its next merge cycle.

```
-retentionPeriod=12
```

**Steady-state guard:** once the 12-month window is active, data older than 12 months
that was never exported is unrecoverable. The daily export keeps `last_exported_day`
within ~1 day of now, far inside the window — but if export breaks and goes unnoticed
for >12 months, days would age out before archiving. Monitor `renogymon-archiver status`
(or alert on `last_exported_day` falling behind) so a stalled export is caught long
before the 12-month horizon.

### Large initial backfill (our case)

The first run backfills VM's *entire* history at once, which is many days of files on
the Pi before any pull. This does **not** risk data loss — VM retention is still full
during the backfill (step 1 above), so VM holds everything until the archive is
verified. The only practical limit is **Pi disk space**:

- **Estimate first:** ~1–3 MB/day → ≈0.4–1 GB/year. Check it against free space on the
  Pi (SD cards are small) before kicking off.
- **If it won't all fit, chunk it:** run `export --max-days N`, then pull (which frees
  the staged files), and repeat until caught up. Peak staging stays ≈ N days.
- **A disk-full mid-backfill is a stall, not a loss:** the failing day's temp file is
  discarded, state does not advance, and export resumes at that day once space is
  freed (by pulling). VM still has the data the whole time.

So for the big-initial-load case the rule is simply: keep VM retention full, drain the
backlog to the archive host (chunking if disk is tight), `renogymon-archiver-puller status`
to confirm no gaps, *then* reduce retention.

## Analysis (Ubuntu Host)

The archive is a plain Parquet corpus — no server, no datasource plugins. Read it
with whatever you already use.

### pandas / pyarrow

`pyarrow`'s dataset API reads the whole directory as one logical table and pushes the
filter down to row groups, so a time- or metric-bounded read only touches what it
needs:

```python
import pyarrow.dataset as ds

dset = ds.dataset("/home/archiver/renogy-archive", format="parquet")
df = dset.to_table(
    filter=(ds.field("metric") == "renogy_soc_percent_value"),
).to_pandas()
# `timestamp` is already tz-aware datetime64[ns, UTC] (no unit conversion needed)

soc = df.set_index("timestamp")["value"]
soc.plot()  # matplotlib
```

Long → wide pivot for multi-metric plots:

```python
wide = df.pivot_table(index="timestamp", columns="metric", values="value")
```

`labels` is a JSON string; expand per-battery with
`df["battery"] = df["labels"].map(lambda s: json.loads(s).get("battery") if s else None)`.

### DuckDB (optional SQL front-end)

```sql
SELECT timestamp, value
FROM read_parquet('/home/archiver/renogy-archive/*.parquet')
WHERE metric = 'renogy_soc_percent_value'
  AND timestamp > now() - INTERVAL '30 days'   -- timestamp is a real TIMESTAMP
ORDER BY timestamp;
```

### File count / layout

The archiver writes flat, filename-dated daily files (~1–3 MB/day, ~365/year). Both
the pyarrow dataset API and DuckDB handle thousands of files fine. If the count ever
becomes a concern:

- **Hive-partition** the remote layout (`year=YYYY/month=MM/renogy_YYYY-MM-DD.parquet`)
  so readers prune whole directories; or
- **Compact** on the host — periodically read a year's dailies and rewrite them as one
  `renogy_YYYY.parquet`, then delete the dailies.

Both are remote-side housekeeping that require no re-export and can be adopted later.
