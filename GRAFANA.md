# Grafana

How to chart renogymon metrics in Grafana.  The dashboard itself lives in
Grafana, not in this repo; this file records the metrics and the queries
behind the panels.

## Metric Names

`renogymon-bms-collector` pushes to VictoriaMetrics with InfluxDB line
protocol, so each stored series name is the collector's measurement name plus
`_value` (e.g. `renogy_current` is stored as `renogy_current_value`).  Every
series has a `battery` label (`<serial>_<address>`); per-cell and per-sensor
series also have `cell` or `sensor`.

The measured metrics are listed in `src/collector/metrics.rs`.  The collector
also derives these:

| Stored name | Unit | Meaning |
|---|---|---|
| `renogy_power_watts_value` | W | `V * I`.  Positive=charging, negative=discharging. |
| `renogy_remaining_energy_wh_value` | Wh | Stored energy, `V * remaining Ah`.  See caveat below. |
| `renogy_charge_energy_wh_value` | Wh | Energy charged since the previous sample of that battery. |
| `renogy_discharge_energy_wh_value` | Wh | Energy discharged since the previous sample of that battery. |

### Charge and Discharge Energy

Each sample carries the energy moved during the interval since the previous
sample, not a running total.  Energy over any range is therefore a plain sum,
with no counter resets or interpolation to worry about.

- Power is integrated trapezoidally between consecutive samples; when power
  changes sign within an interval the area is split at the zero crossing.
- Intervals longer than 4 polls (60 s at the default 15 s poll) are not
  integrated, and the first sample after a gap or restart carries no energy.
  Dropouts therefore undercount rather than hold a stale reading across the
  gap.  (MetricsQL `integrate()` would hold the last value across a gap.)
- Current is sampled every poll, so load changes faster than the poll interval
  are aliased; the error averages out over long ranges but not over short ones.
- These are pushed only; they are not on the `/metrics` endpoint, since a
  per-sample value is meaningless to a scraper on a different interval.

### Remaining Energy Caveat

`renogy_remaining_energy_wh_value` is not integrated by renogymon, so it needs
no initial condition.  It converts the BMS's own remaining-capacity register
(the BMS's own charge counter; how and when the BMS recalibrates it is not
documented) from Ah to Wh, and inherits whatever drift that counter has.  The
conversion multiplies by the pack voltage at that instant.  Charging raises the voltage and heavy loads sag it,
so the value steps when loads switch even though stored charge barely changes.
For LiFePO4 the error is a few percent.  Smooth it for display, or use
`renogy_remaining_capacity_ah_value` and SOC when precision matters.

## Panel Queries

`$__range` and `$__interval` are Grafana variables.  Unless noted, queries
work in both PromQL and MetricsQL.

### Power (time series)

```promql
renogy_power_watts_value
```

Smoothed, to hide per-poll load noise:

```promql
avg_over_time(renogy_power_watts_value[5m])
```

### Energy over the dashboard range (stat)

Set the query type to *Instant*.

```promql
sum by (battery) (sum_over_time(renogy_charge_energy_wh_value[$__range]))
sum by (battery) (sum_over_time(renogy_discharge_energy_wh_value[$__range]))
```

Net (positive=battery gained energy):

```promql
sum by (battery) (sum_over_time(renogy_charge_energy_wh_value[$__range]))
  - sum by (battery) (sum_over_time(renogy_discharge_energy_wh_value[$__range]))
```

Round-trip ratio over a long range (e.g. 30d; meaningful only when the start
and end SOC are similar):

```promql
sum(sum_over_time(renogy_discharge_energy_wh_value[$__range]))
  / sum(sum_over_time(renogy_charge_energy_wh_value[$__range]))
```

### Daily energy (bar chart)

Use the time series panel with *Bars* draw style, and set the query options
*Min interval* to `1d` so `$__interval` is one day.

```promql
sum by (battery) (sum_over_time(renogy_charge_energy_wh_value[$__interval]))
sum by (battery) (sum_over_time(renogy_discharge_energy_wh_value[$__interval]))
```

To draw discharge below the axis, add a field override on the discharge series:
*Transform* -> *Negative Y*.

Buckets end at each evaluation step, which Grafana aligns to UTC.  For
local-midnight days, set the dashboard timezone and check the step alignment
of your datasource.

### Stored energy (time series)

```promql
avg_over_time(renogy_remaining_energy_wh_value[5m])
```

## Backfilling History

Series recorded before the derived metrics existed can be recomputed from
`renogy_module_voltage_value`, `renogy_current_value` and
`renogy_remaining_capacity_ah_value`: read them with VictoriaMetrics
`/api/v1/export`, apply the same calculations (including the gap limit), and
write the results with `/api/v1/import`.  Samples from one poll share a
timestamp, so voltage and current join exactly.  Skip timestamps that already
have a derived value so reruns and overlap with the live collector do not
double count, then call `/internal/resetRollupResultCache` so Grafana does not
serve cached results.
