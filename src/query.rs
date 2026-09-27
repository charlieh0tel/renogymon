use crate::alarm::CellTemperatureAlarms;
use crate::alarm::CellVoltageAlarms;
use crate::alarm::ChargeDischargeStatus;
use crate::alarm::OtherAlarmInfo;
use crate::alarm::Status1;
use crate::alarm::Status2;
use crate::alarm::Status3;
use crate::registers::Register;
use crate::registers::Value;
use crate::transport::Transport;
use chrono::DateTime;
use chrono::Utc;
use uom::si::electric_current::ampere;
use uom::si::electric_potential::volt;
use uom::si::thermodynamic_temperature::degree_celsius;

const STATUS1_NON_ALARM_FLAGS: Status1 = Status1::CHARGE_MOSFET
    .union(Status1::DISCHARGE_MOSFET)
    .union(Status1::USING_BATTERY_MODULE_POWER);

const STATUS2_NON_ALARM_FLAGS: Status2 = Status2::EFFECTIVE_CHARGE_CURRENT
    .union(Status2::EFFECTIVE_DISCHARGE_CURRENT)
    .union(Status2::HEATER_ON)
    .union(Status2::FULLY_CHARGED);

#[derive(Clone, Debug)]
pub struct BatteryInfo {
    pub timestamp: DateTime<Utc>,
    pub serial: String,
    pub model: String,
    pub software_version: String,
    pub manufacturer: String,
    pub cell_count: u32,
    pub cell_voltages: Vec<f32>,
    pub cell_temperatures: Vec<f32>,
    pub bms_temperature: Option<f32>,
    pub environment_temperatures: Vec<f32>,
    pub heater_temperatures: Vec<f32>,
    pub module_voltage: f32,
    pub current: f32,
    pub remaining_capacity: f32,
    pub total_capacity: f32,
    pub soc_percent: f32,
    pub cycle_count: u32,
    pub charge_voltage_limit: Option<f32>,
    pub discharge_voltage_limit: Option<f32>,
    pub charge_current_limit: Option<f32>,
    pub discharge_current_limit: Option<f32>,
    pub status1: Option<Status1>,
    pub status2: Option<Status2>,
    pub status3: Option<Status3>,
    pub other_alarm_info: Option<OtherAlarmInfo>,
    pub cell_voltage_alarms: Option<CellVoltageAlarms>,
    pub cell_temperature_alarms: Option<CellTemperatureAlarms>,
    pub charge_discharge_status: Option<ChargeDischargeStatus>,
}

impl BatteryInfo {
    /// Battery power in watts (positive=charging, negative=discharging).
    #[must_use]
    pub fn power_watts(&self) -> f32 {
        self.module_voltage * self.current
    }

    /// Stored energy in watt-hours, approximated at the present pack voltage.
    #[must_use]
    pub fn remaining_energy_wh(&self) -> f32 {
        self.module_voltage * self.remaining_capacity
    }

    /// Returns a list of active alarm names from all status registers.
    ///
    /// Excludes non-alarm flags like MOSFET states, FULLY_CHARGED, HEATER_ON, etc.
    #[must_use]
    pub fn active_alarms(&self) -> Vec<&'static str> {
        let mut alarms = Vec::new();

        if let Some(s1) = self.status1 {
            for (name, flag) in s1.iter_names() {
                if !STATUS1_NON_ALARM_FLAGS.contains(flag) {
                    alarms.push(name);
                }
            }
        }

        if let Some(s2) = self.status2 {
            for (name, flag) in s2.iter_names() {
                if !STATUS2_NON_ALARM_FLAGS.contains(flag) {
                    alarms.push(name);
                }
            }
        }

        if let Some(s3) = self.status3 {
            for (name, _) in s3.iter_names() {
                alarms.push(name);
            }
        }

        if let Some(other) = self.other_alarm_info {
            for (name, _) in other.iter_names() {
                alarms.push(name);
            }
        }

        alarms
    }

    /// Returns true if any alarms are active.
    #[must_use]
    pub fn has_alarms(&self) -> bool {
        !self.active_alarms().is_empty()
    }
}

pub async fn query_battery<T: Transport>(transport: &mut T, addr: u8) -> Option<BatteryInfo> {
    let serial = read_string(transport, addr, Register::SnNumber).await?;
    let model = read_string(transport, addr, Register::BatteryName)
        .await
        .unwrap_or_default();
    let software_version = read_string(transport, addr, Register::SoftwareVersion)
        .await
        .unwrap_or_default();
    let manufacturer = read_string(transport, addr, Register::ManufacturerName)
        .await
        .unwrap_or_default();

    let cell_count = read_integer(transport, addr, Register::CellCount).await?;

    let mut cell_voltages = Vec::with_capacity(cell_count.min(16) as usize);
    for i in 1..=cell_count.min(16) {
        if let Some(v) = read_voltage(transport, addr, Register::CellVoltage(i as u8)).await {
            cell_voltages.push(v);
        }
    }

    let module_voltage = read_voltage(transport, addr, Register::ModuleVoltage)
        .await
        .unwrap_or(0.0);
    let current = read_current(transport, addr, Register::Current)
        .await
        .unwrap_or(0.0);
    let remaining_capacity = read_current(transport, addr, Register::RemainingCapacity)
        .await
        .unwrap_or(0.0);
    let total_capacity = read_current(transport, addr, Register::TotalCapacity)
        .await
        .unwrap_or(0.0);

    let soc_percent = if total_capacity > 0.0 {
        (remaining_capacity / total_capacity) * 100.0
    } else {
        0.0
    };

    let cycle_count = read_integer(transport, addr, Register::CycleNumber)
        .await
        .unwrap_or(0);

    let cell_temp_count = read_integer(transport, addr, Register::CellTemperatureCount)
        .await
        .unwrap_or(0);

    let mut cell_temperatures = Vec::with_capacity(cell_temp_count.min(16) as usize);
    for i in 1..=cell_temp_count.min(16) {
        if let Some(t) = read_temperature(transport, addr, Register::CellTemperature(i as u8)).await
        {
            cell_temperatures.push(t);
        }
    }

    let bms_temperature = read_temperature(transport, addr, Register::BmsTemperature).await;

    let env_temp_count = read_integer(transport, addr, Register::EnvironmentTemperatureCount)
        .await
        .unwrap_or(0);
    let mut environment_temperatures = Vec::with_capacity(env_temp_count.min(2) as usize);
    for i in 1..=env_temp_count.min(2) {
        if let Some(t) =
            read_temperature(transport, addr, Register::EnvironmentTemperature(i as u8)).await
        {
            environment_temperatures.push(t);
        }
    }

    let heater_temp_count = read_integer(transport, addr, Register::HeaterTemperatureCount)
        .await
        .unwrap_or(0);
    let mut heater_temperatures = Vec::with_capacity(heater_temp_count.min(2) as usize);
    for i in 1..=heater_temp_count.min(2) {
        if let Some(t) =
            read_temperature(transport, addr, Register::HeaterTemperature(i as u8)).await
        {
            heater_temperatures.push(t);
        }
    }

    let charge_voltage_limit = read_voltage(transport, addr, Register::ChargeVoltageLimit).await;
    let discharge_voltage_limit =
        read_voltage(transport, addr, Register::DischargeVoltageLimit).await;
    let charge_current_limit = read_current(transport, addr, Register::ChargeCurrentLimit).await;
    let discharge_current_limit =
        read_current(transport, addr, Register::DischargeCurrentLimit).await;

    let status1 = read_status1(transport, addr).await;
    let status2 = read_status2(transport, addr).await;
    let status3 = read_status3(transport, addr).await;
    let other_alarm_info = read_other_alarm_info(transport, addr).await;
    let cell_voltage_alarms = read_cell_voltage_alarms(transport, addr).await;
    let cell_temperature_alarms = read_cell_temperature_alarms(transport, addr).await;
    let charge_discharge_status = read_charge_discharge_status(transport, addr).await;

    Some(BatteryInfo {
        timestamp: Utc::now(),
        serial,
        model,
        software_version,
        manufacturer,
        cell_count,
        cell_voltages,
        cell_temperatures,
        bms_temperature,
        environment_temperatures,
        heater_temperatures,
        module_voltage,
        current,
        remaining_capacity,
        total_capacity,
        soc_percent,
        cycle_count,
        charge_voltage_limit,
        discharge_voltage_limit,
        charge_current_limit,
        discharge_current_limit,
        status1,
        status2,
        status3,
        other_alarm_info,
        cell_voltage_alarms,
        cell_temperature_alarms,
        charge_discharge_status,
    })
}

async fn read_register<T: Transport>(
    transport: &mut T,
    addr: u8,
    register: Register,
) -> Option<Value> {
    let regs = transport
        .read_holding_registers(addr, register.address(), register.quantity())
        .await
        .ok()?;
    Some(register.parse_registers(&regs))
}

async fn read_string<T: Transport>(
    transport: &mut T,
    addr: u8,
    register: Register,
) -> Option<String> {
    read_register(transport, addr, register)
        .await?
        .as_string()
        .map(|s| s.trim_matches('\0').to_string())
}

async fn read_integer<T: Transport>(
    transport: &mut T,
    addr: u8,
    register: Register,
) -> Option<u32> {
    read_register(transport, addr, register).await?.as_integer()
}

async fn read_voltage<T: Transport>(
    transport: &mut T,
    addr: u8,
    register: Register,
) -> Option<f32> {
    read_register(transport, addr, register)
        .await?
        .as_voltage()
        .map(|v| v.get::<volt>())
}

async fn read_current<T: Transport>(
    transport: &mut T,
    addr: u8,
    register: Register,
) -> Option<f32> {
    read_register(transport, addr, register)
        .await?
        .as_current()
        .map(|c| c.get::<ampere>())
}

async fn read_temperature<T: Transport>(
    transport: &mut T,
    addr: u8,
    register: Register,
) -> Option<f32> {
    read_register(transport, addr, register)
        .await?
        .as_temperature()
        .map(|t| t.get::<degree_celsius>())
}

async fn read_status1<T: Transport>(transport: &mut T, addr: u8) -> Option<Status1> {
    read_register(transport, addr, Register::Status1)
        .await?
        .as_status1()
}

async fn read_status2<T: Transport>(transport: &mut T, addr: u8) -> Option<Status2> {
    read_register(transport, addr, Register::Status2)
        .await?
        .as_status2()
}

async fn read_status3<T: Transport>(transport: &mut T, addr: u8) -> Option<Status3> {
    read_register(transport, addr, Register::Status3)
        .await?
        .as_status3()
}

async fn read_other_alarm_info<T: Transport>(
    transport: &mut T,
    addr: u8,
) -> Option<OtherAlarmInfo> {
    read_register(transport, addr, Register::OtherAlarmInfo)
        .await?
        .as_other_alarm_info()
}

async fn read_cell_voltage_alarms<T: Transport>(
    transport: &mut T,
    addr: u8,
) -> Option<CellVoltageAlarms> {
    read_register(transport, addr, Register::CellVoltageAlarmInfo)
        .await?
        .as_cell_voltage_alarms()
}

async fn read_cell_temperature_alarms<T: Transport>(
    transport: &mut T,
    addr: u8,
) -> Option<CellTemperatureAlarms> {
    read_register(transport, addr, Register::CellTemperatureAlarmInfo)
        .await?
        .as_cell_temperature_alarms()
}

async fn read_charge_discharge_status<T: Transport>(
    transport: &mut T,
    addr: u8,
) -> Option<ChargeDischargeStatus> {
    read_register(transport, addr, Register::ChargeDischargeStatus)
        .await?
        .as_charge_discharge_status()
}
