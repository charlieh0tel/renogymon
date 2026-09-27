use crate::collector::energy::Sample;
use crate::query::BatteryInfo;
use influxdb_line_protocol::LineProtocolBuilder;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use std::sync::atomic::AtomicU64;

fn bool_to_f64(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct BatteryLabels {
    pub battery: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct CellLabels {
    pub battery: String,
    pub cell: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct SensorLabels {
    pub battery: String,
    pub sensor: String,
}

#[derive(Default)]
pub struct PrometheusMetrics {
    pub cell_voltage: Family<CellLabels, Gauge<f64, AtomicU64>>,
    pub cell_temperature: Family<CellLabels, Gauge<f64, AtomicU64>>,
    pub bms_temperature: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub environment_temperature: Family<SensorLabels, Gauge<f64, AtomicU64>>,
    pub heater_temperature: Family<SensorLabels, Gauge<f64, AtomicU64>>,
    pub module_voltage: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub current: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub power_watts: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub remaining_capacity_ah: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub total_capacity_ah: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub remaining_energy_wh: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub soc_percent: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub cycle_count: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub charge_voltage_limit: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub discharge_voltage_limit: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub charge_current_limit: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub discharge_current_limit: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub status1: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub status2: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub status3: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub other_alarm_info: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub charge_mosfet_on: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub discharge_mosfet_on: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub charge_enabled: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub discharge_enabled: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub fully_charged: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
    pub heater_on: Family<BatteryLabels, Gauge<f64, AtomicU64>>,
}

impl PrometheusMetrics {
    pub fn register(&self, registry: &mut Registry) {
        registry.register(
            "renogy_cell_voltage",
            "Individual cell voltage in volts",
            self.cell_voltage.clone(),
        );
        registry.register(
            "renogy_cell_temperature",
            "Individual cell temperature in celsius",
            self.cell_temperature.clone(),
        );
        registry.register(
            "renogy_bms_temperature",
            "BMS board temperature in celsius",
            self.bms_temperature.clone(),
        );
        registry.register(
            "renogy_environment_temperature",
            "Environment temperature sensor in celsius",
            self.environment_temperature.clone(),
        );
        registry.register(
            "renogy_heater_temperature",
            "Heater temperature sensor in celsius",
            self.heater_temperature.clone(),
        );
        registry.register(
            "renogy_module_voltage",
            "Module/pack voltage in volts",
            self.module_voltage.clone(),
        );
        registry.register(
            "renogy_current",
            "Battery current in amps (positive=charging, negative=discharging)",
            self.current.clone(),
        );
        registry.register(
            "renogy_power_watts",
            "Battery power in watts (positive=charging, negative=discharging)",
            self.power_watts.clone(),
        );
        registry.register(
            "renogy_remaining_capacity_ah",
            "Remaining capacity in amp-hours",
            self.remaining_capacity_ah.clone(),
        );
        registry.register(
            "renogy_total_capacity_ah",
            "Total capacity in amp-hours",
            self.total_capacity_ah.clone(),
        );
        registry.register(
            "renogy_remaining_energy_wh",
            "Stored energy in watt-hours at present pack voltage",
            self.remaining_energy_wh.clone(),
        );
        registry.register(
            "renogy_soc_percent",
            "State of charge percentage",
            self.soc_percent.clone(),
        );
        registry.register(
            "renogy_cycle_count",
            "Number of charge cycles",
            self.cycle_count.clone(),
        );
        registry.register(
            "renogy_charge_voltage_limit",
            "Charge voltage limit in volts",
            self.charge_voltage_limit.clone(),
        );
        registry.register(
            "renogy_discharge_voltage_limit",
            "Discharge voltage limit in volts",
            self.discharge_voltage_limit.clone(),
        );
        registry.register(
            "renogy_charge_current_limit",
            "Charge current limit in amps",
            self.charge_current_limit.clone(),
        );
        registry.register(
            "renogy_discharge_current_limit",
            "Discharge current limit in amps",
            self.discharge_current_limit.clone(),
        );
        registry.register(
            "renogy_status1",
            "Status1 register raw value (protection flags)",
            self.status1.clone(),
        );
        registry.register(
            "renogy_status2",
            "Status2 register raw value (warning flags)",
            self.status2.clone(),
        );
        registry.register(
            "renogy_status3",
            "Status3 register raw value (cell voltage errors)",
            self.status3.clone(),
        );
        registry.register(
            "renogy_other_alarm_info",
            "Other alarm info register raw value",
            self.other_alarm_info.clone(),
        );
        registry.register(
            "renogy_charge_mosfet_on",
            "Charge MOSFET state (1=on, 0=off)",
            self.charge_mosfet_on.clone(),
        );
        registry.register(
            "renogy_discharge_mosfet_on",
            "Discharge MOSFET state (1=on, 0=off)",
            self.discharge_mosfet_on.clone(),
        );
        registry.register(
            "renogy_charge_enabled",
            "Charge enabled (1=yes, 0=no)",
            self.charge_enabled.clone(),
        );
        registry.register(
            "renogy_discharge_enabled",
            "Discharge enabled (1=yes, 0=no)",
            self.discharge_enabled.clone(),
        );
        registry.register(
            "renogy_fully_charged",
            "Battery fully charged (1=yes, 0=no)",
            self.fully_charged.clone(),
        );
        registry.register(
            "renogy_heater_on",
            "Heater state (1=on, 0=off)",
            self.heater_on.clone(),
        );
    }

    pub fn update(&self, info: &BatteryInfo) {
        use crate::alarm::ChargeDischargeStatus;
        use crate::alarm::Status1;
        use crate::alarm::Status2;

        let serial = &info.serial;
        let battery_labels = BatteryLabels {
            battery: serial.clone(),
        };

        for (i, &voltage) in info.cell_voltages.iter().enumerate() {
            let labels = CellLabels {
                battery: serial.clone(),
                cell: (i + 1).to_string(),
            };
            self.cell_voltage.get_or_create(&labels).set(voltage as f64);
        }

        for (i, &temp) in info.cell_temperatures.iter().enumerate() {
            let labels = CellLabels {
                battery: serial.clone(),
                cell: (i + 1).to_string(),
            };
            self.cell_temperature
                .get_or_create(&labels)
                .set(temp as f64);
        }

        if let Some(temp) = info.bms_temperature {
            self.bms_temperature
                .get_or_create(&battery_labels)
                .set(temp as f64);
        }

        for (i, &temp) in info.environment_temperatures.iter().enumerate() {
            let labels = SensorLabels {
                battery: serial.clone(),
                sensor: (i + 1).to_string(),
            };
            self.environment_temperature
                .get_or_create(&labels)
                .set(temp as f64);
        }

        for (i, &temp) in info.heater_temperatures.iter().enumerate() {
            let labels = SensorLabels {
                battery: serial.clone(),
                sensor: (i + 1).to_string(),
            };
            self.heater_temperature
                .get_or_create(&labels)
                .set(temp as f64);
        }

        self.module_voltage
            .get_or_create(&battery_labels)
            .set(info.module_voltage as f64);
        self.current
            .get_or_create(&battery_labels)
            .set(info.current as f64);
        self.power_watts
            .get_or_create(&battery_labels)
            .set(info.power_watts() as f64);
        self.remaining_capacity_ah
            .get_or_create(&battery_labels)
            .set(info.remaining_capacity as f64);
        self.total_capacity_ah
            .get_or_create(&battery_labels)
            .set(info.total_capacity as f64);
        self.remaining_energy_wh
            .get_or_create(&battery_labels)
            .set(info.remaining_energy_wh() as f64);
        self.soc_percent
            .get_or_create(&battery_labels)
            .set(info.soc_percent as f64);
        self.cycle_count
            .get_or_create(&battery_labels)
            .set(info.cycle_count as f64);

        if let Some(limit) = info.charge_voltage_limit {
            self.charge_voltage_limit
                .get_or_create(&battery_labels)
                .set(limit as f64);
        }
        if let Some(limit) = info.discharge_voltage_limit {
            self.discharge_voltage_limit
                .get_or_create(&battery_labels)
                .set(limit as f64);
        }
        if let Some(limit) = info.charge_current_limit {
            self.charge_current_limit
                .get_or_create(&battery_labels)
                .set(limit as f64);
        }
        if let Some(limit) = info.discharge_current_limit {
            self.discharge_current_limit
                .get_or_create(&battery_labels)
                .set(limit as f64);
        }

        if let Some(s) = info.status1 {
            self.status1
                .get_or_create(&battery_labels)
                .set(s.bits() as f64);
            self.charge_mosfet_on
                .get_or_create(&battery_labels)
                .set(bool_to_f64(s.contains(Status1::CHARGE_MOSFET)));
            self.discharge_mosfet_on
                .get_or_create(&battery_labels)
                .set(bool_to_f64(s.contains(Status1::DISCHARGE_MOSFET)));
        }

        if let Some(s) = info.status2 {
            self.status2
                .get_or_create(&battery_labels)
                .set(s.bits() as f64);
            self.fully_charged
                .get_or_create(&battery_labels)
                .set(bool_to_f64(s.contains(Status2::FULLY_CHARGED)));
            self.heater_on
                .get_or_create(&battery_labels)
                .set(bool_to_f64(s.contains(Status2::HEATER_ON)));
        }

        if let Some(s) = info.status3 {
            self.status3
                .get_or_create(&battery_labels)
                .set(s.bits() as f64);
        }

        if let Some(s) = info.other_alarm_info {
            self.other_alarm_info
                .get_or_create(&battery_labels)
                .set(s.bits() as f64);
        }

        if let Some(s) = info.charge_discharge_status {
            self.charge_enabled
                .get_or_create(&battery_labels)
                .set(bool_to_f64(
                    s.contains(ChargeDischargeStatus::CHARGE_ENABLE),
                ));
            self.discharge_enabled
                .get_or_create(&battery_labels)
                .set(bool_to_f64(
                    s.contains(ChargeDischargeStatus::DISCHARGE_ENABLE),
                ));
        }
    }
}

pub fn batch_to_influx(samples: &[Sample]) -> String {
    use crate::alarm::ChargeDischargeStatus;
    use crate::alarm::Status1;
    use crate::alarm::Status2;

    macro_rules! measurement {
        ($b:expr, $name:expr, $serial:expr, $value:expr, $ts:expr) => {
            $b.measurement($name)
                .tag("battery", $serial)
                .field("value", $value)
                .timestamp($ts)
                .close_line()
        };
    }

    macro_rules! cell_measurement {
        ($b:expr, $name:expr, $serial:expr, $cell:expr, $value:expr, $ts:expr) => {
            $b.measurement($name)
                .tag("battery", $serial)
                .tag("cell", $cell)
                .field("value", $value)
                .timestamp($ts)
                .close_line()
        };
    }

    macro_rules! sensor_measurement {
        ($b:expr, $name:expr, $serial:expr, $sensor:expr, $value:expr, $ts:expr) => {
            $b.measurement($name)
                .tag("battery", $serial)
                .tag("sensor", $sensor)
                .field("value", $value)
                .timestamp($ts)
                .close_line()
        };
    }

    let mut builder = LineProtocolBuilder::new();

    for sample in samples {
        let info = &sample.info;
        let ts = info.timestamp.timestamp_nanos_opt().unwrap_or(0);
        let serial = &info.serial;

        for (i, &voltage) in info.cell_voltages.iter().enumerate() {
            let cell = (i + 1).to_string();
            builder = cell_measurement!(
                builder,
                "renogy_cell_voltage",
                serial,
                &cell,
                voltage as f64,
                ts
            );
        }

        for (i, &temp) in info.cell_temperatures.iter().enumerate() {
            let cell = (i + 1).to_string();
            builder = cell_measurement!(
                builder,
                "renogy_cell_temperature",
                serial,
                &cell,
                temp as f64,
                ts
            );
        }

        if let Some(temp) = info.bms_temperature {
            builder = measurement!(builder, "renogy_bms_temperature", serial, temp as f64, ts);
        }

        for (i, &temp) in info.environment_temperatures.iter().enumerate() {
            let sensor = (i + 1).to_string();
            builder = sensor_measurement!(
                builder,
                "renogy_environment_temperature",
                serial,
                &sensor,
                temp as f64,
                ts
            );
        }

        for (i, &temp) in info.heater_temperatures.iter().enumerate() {
            let sensor = (i + 1).to_string();
            builder = sensor_measurement!(
                builder,
                "renogy_heater_temperature",
                serial,
                &sensor,
                temp as f64,
                ts
            );
        }

        builder = measurement!(
            builder,
            "renogy_module_voltage",
            serial,
            info.module_voltage as f64,
            ts
        );
        builder = measurement!(builder, "renogy_current", serial, info.current as f64, ts);
        builder = measurement!(
            builder,
            "renogy_power_watts",
            serial,
            info.power_watts() as f64,
            ts
        );
        if let Some(energy) = sample.energy {
            builder = measurement!(
                builder,
                "renogy_charge_energy_wh",
                serial,
                energy.charge_wh,
                ts
            );
            builder = measurement!(
                builder,
                "renogy_discharge_energy_wh",
                serial,
                energy.discharge_wh,
                ts
            );
        }
        builder = measurement!(
            builder,
            "renogy_remaining_capacity_ah",
            serial,
            info.remaining_capacity as f64,
            ts
        );
        builder = measurement!(
            builder,
            "renogy_total_capacity_ah",
            serial,
            info.total_capacity as f64,
            ts
        );
        builder = measurement!(
            builder,
            "renogy_remaining_energy_wh",
            serial,
            info.remaining_energy_wh() as f64,
            ts
        );
        builder = measurement!(
            builder,
            "renogy_soc_percent",
            serial,
            info.soc_percent as f64,
            ts
        );
        builder = measurement!(
            builder,
            "renogy_cycle_count",
            serial,
            info.cycle_count as f64,
            ts
        );

        if let Some(limit) = info.charge_voltage_limit {
            builder = measurement!(
                builder,
                "renogy_charge_voltage_limit",
                serial,
                limit as f64,
                ts
            );
        }
        if let Some(limit) = info.discharge_voltage_limit {
            builder = measurement!(
                builder,
                "renogy_discharge_voltage_limit",
                serial,
                limit as f64,
                ts
            );
        }
        if let Some(limit) = info.charge_current_limit {
            builder = measurement!(
                builder,
                "renogy_charge_current_limit",
                serial,
                limit as f64,
                ts
            );
        }
        if let Some(limit) = info.discharge_current_limit {
            builder = measurement!(
                builder,
                "renogy_discharge_current_limit",
                serial,
                limit as f64,
                ts
            );
        }

        if let Some(s) = info.status1 {
            builder = measurement!(builder, "renogy_status1", serial, s.bits() as f64, ts);
            builder = measurement!(
                builder,
                "renogy_charge_mosfet_on",
                serial,
                bool_to_f64(s.contains(Status1::CHARGE_MOSFET)),
                ts
            );
            builder = measurement!(
                builder,
                "renogy_discharge_mosfet_on",
                serial,
                bool_to_f64(s.contains(Status1::DISCHARGE_MOSFET)),
                ts
            );
        }

        if let Some(s) = info.status2 {
            builder = measurement!(builder, "renogy_status2", serial, s.bits() as f64, ts);
            builder = measurement!(
                builder,
                "renogy_fully_charged",
                serial,
                bool_to_f64(s.contains(Status2::FULLY_CHARGED)),
                ts
            );
            builder = measurement!(
                builder,
                "renogy_heater_on",
                serial,
                bool_to_f64(s.contains(Status2::HEATER_ON)),
                ts
            );
        }

        if let Some(s) = info.status3 {
            builder = measurement!(builder, "renogy_status3", serial, s.bits() as f64, ts);
        }

        if let Some(s) = info.other_alarm_info {
            builder = measurement!(
                builder,
                "renogy_other_alarm_info",
                serial,
                s.bits() as f64,
                ts
            );
        }

        if let Some(s) = info.charge_discharge_status {
            builder = measurement!(
                builder,
                "renogy_charge_enabled",
                serial,
                bool_to_f64(s.contains(ChargeDischargeStatus::CHARGE_ENABLE)),
                ts
            );
            builder = measurement!(
                builder,
                "renogy_discharge_enabled",
                serial,
                bool_to_f64(s.contains(ChargeDischargeStatus::DISCHARGE_ENABLE)),
                ts
            );
        }
    }

    String::from_utf8(builder.build()).expect("line protocol should be valid UTF-8")
}
