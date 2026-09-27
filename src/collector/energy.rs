use crate::query::BatteryInfo;
use chrono::DateTime;
use chrono::Utc;
use std::collections::HashMap;
use std::time::Duration;

const SECONDS_PER_HOUR: f64 = 3600.0;

/// Energy moved through the battery terminals between two consecutive samples,
/// split by direction.  Both fields are non-negative.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnergyDelta {
    pub charge_wh: f64,
    pub discharge_wh: f64,
}

/// A collected sample plus the energy moved since the previous sample of the
/// same battery (`None` for the first sample or after a gap).
#[derive(Clone, Debug)]
pub struct Sample {
    pub info: BatteryInfo,
    pub energy: Option<EnergyDelta>,
}

/// Integrates battery power between polls.  Holds the previous timestamp and
/// power (W) for each battery, keyed by serial.
pub struct EnergyTracker {
    max_gap: Duration,
    previous: HashMap<String, (DateTime<Utc>, f64)>,
}

impl EnergyTracker {
    /// Intervals longer than `max_gap` are not integrated, so a dropout does
    /// not hold a stale power reading across the gap.
    pub fn new(max_gap: Duration) -> Self {
        Self {
            max_gap,
            previous: HashMap::new(),
        }
    }

    pub fn observe(&mut self, info: BatteryInfo) -> Sample {
        let power = f64::from(info.power_watts());
        let energy = self
            .previous
            .insert(info.serial.clone(), (info.timestamp, power))
            .and_then(|(t0, p0)| {
                let dt = (info.timestamp - t0).to_std().ok()?;
                (dt <= self.max_gap).then(|| trapezoid(p0, power, dt))
            });
        Sample { info, energy }
    }
}

/// Trapezoidal integral of power from `p0` to `p1` over `dt`, splitting the
/// area at the zero crossing when the sign changes.
fn trapezoid(p0: f64, p1: f64, dt: Duration) -> EnergyDelta {
    let hours = dt.as_secs_f64() / SECONDS_PER_HOUR;
    let (positive, negative) = if p0 * p1 >= 0.0 {
        let area = (p0 + p1) / 2.0 * hours;
        (area, -area)
    } else {
        // Linear interpolation crosses zero at fraction p0 / (p0 - p1) of dt.
        let crossing = p0 / (p0 - p1);
        let first = p0 / 2.0 * crossing * hours;
        let second = p1 / 2.0 * (1.0 - crossing) * hours;
        (first.max(second), -first.min(second))
    };
    EnergyDelta {
        charge_wh: positive.max(0.0),
        discharge_wh: negative.max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);
    const MAX_GAP: Duration = Duration::from_secs(60);

    fn info(seconds: i64, volts: f32, amps: f32) -> BatteryInfo {
        BatteryInfo {
            timestamp: DateTime::from_timestamp(seconds, 0).unwrap(),
            serial: "SN1".to_string(),
            model: String::new(),
            software_version: String::new(),
            manufacturer: String::new(),
            cell_count: 0,
            cell_voltages: Vec::new(),
            cell_temperatures: Vec::new(),
            bms_temperature: None,
            environment_temperatures: Vec::new(),
            heater_temperatures: Vec::new(),
            module_voltage: volts,
            current: amps,
            remaining_capacity: 0.0,
            total_capacity: 0.0,
            soc_percent: 0.0,
            cycle_count: 0,
            charge_voltage_limit: None,
            discharge_voltage_limit: None,
            charge_current_limit: None,
            discharge_current_limit: None,
            status1: None,
            status2: None,
            status3: None,
            other_alarm_info: None,
            cell_voltage_alarms: None,
            cell_temperature_alarms: None,
            charge_discharge_status: None,
        }
    }

    #[test]
    fn tracker_skips_first_sample_and_gaps() {
        let mut tracker = EnergyTracker::new(MAX_GAP);
        assert_eq!(tracker.observe(info(0, 12.0, -10.0)).energy, None);
        // 36 s at -120 W = 1.2 Wh discharged.
        let delta = tracker.observe(info(36, 12.0, -10.0)).energy.unwrap();
        assert_eq!(delta.charge_wh, 0.0);
        assert!((delta.discharge_wh - 1.2).abs() < 1e-9);
        assert_eq!(tracker.observe(info(1000, 12.0, -10.0)).energy, None);
        assert!(tracker.observe(info(1015, 12.0, -10.0)).energy.is_some());
    }

    #[test]
    fn same_sign_is_trapezoid() {
        assert_eq!(
            trapezoid(100.0, 200.0, HOUR),
            EnergyDelta {
                charge_wh: 150.0,
                discharge_wh: 0.0
            }
        );
        assert_eq!(
            trapezoid(-100.0, -200.0, HOUR),
            EnergyDelta {
                charge_wh: 0.0,
                discharge_wh: 150.0
            }
        );
    }

    #[test]
    fn sign_change_splits_at_crossing() {
        // Crosses zero at 1/4 hour: 100 W * 0.25 h / 2 and 300 W * 0.75 h / 2.
        let delta = trapezoid(100.0, -300.0, HOUR);
        assert!((delta.charge_wh - 12.5).abs() < 1e-9);
        assert!((delta.discharge_wh - 112.5).abs() < 1e-9);
    }
}
