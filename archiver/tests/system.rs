//! Hermetic Tier 1 system test (see SYSTEM_TEST.md).
//!
//! Emulated battery -> real `query_battery` -> real collector influx encoding ->
//! in-process mock VictoriaMetrics -> real `renogymon-archiver` export -> Parquet. Asserts
//! the full set of served values round-trips and that a foreign (non-`renogy_`) series
//! is excluded. Gated with `#[ignore]`; run with
//! `cargo test -p renogymon-archiver --test system -- --ignored`.

mod common;

use std::collections::HashSet;
use std::fs::File;

use arrow::array::Array;
use arrow::array::Float64Array;
use arrow::array::StringArray;
use common::MockVm;
use common::day_noon_ms;
use common::days_ago;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use renogy::collector::energy::Sample;
use renogy::collector::metrics::batch_to_influx;
use renogy::emulator::EmulatedBattery;
use renogy::query::query_battery;
use renogy::registers::Register;
use renogymon_archiver::archiver::ExportConfig;
use renogymon_archiver::archiver::run_export;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "system test; run explicitly with --ignored"]
async fn battery_through_collector_to_parquet() {
    let vm = MockVm::start().await;

    // Emulated battery -> real query_battery.
    let addr = 0x30;
    let mut bms = EmulatedBattery::new(addr);
    bms.set_string(Register::SnNumber, "SN1234").unwrap();
    bms.set_integer(Register::CellCount, 4).unwrap();
    for cell in 1..=4 {
        bms.set_voltage(Register::CellVoltage(cell), 3.3).unwrap();
    }
    bms.set_voltage(Register::ModuleVoltage, 13.2).unwrap();
    bms.set_current(Register::Current, -5.0).unwrap();
    bms.set_current(Register::RemainingCapacity, 50.0).unwrap();
    bms.set_current(Register::TotalCapacity, 100.0).unwrap();

    let mut info = query_battery(&mut bms, addr).await.expect("battery info");

    // Stamp into yesterday so the archiver (which exports through today-1) picks it up.
    let yesterday = days_ago(1);
    info.timestamp = yesterday.and_hms_opt(12, 0, 0).unwrap().and_utc();

    // Real collector encoding -> mock VM /write.
    reqwest::Client::new()
        .post(format!("{}/write", vm.base))
        .body(batch_to_influx(&[Sample { info, energy: None }]))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // A foreign (non-renogy) series that must NOT appear in the archive.
    vm.add("foo_value", &[], day_noon_ms(yesterday), 99.0);

    // Real archiver export -> Parquet.
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    let cfg = ExportConfig {
        vm_addr: vm.base.clone(),
        staging_dir: staging.clone(),
        state_file: tmp.path().join("state.json"),
        start_date: Some(yesterday),
        max_days: None,
    };
    run_export(&cfg).await.expect("export");

    let path = staging.join(format!("renogy_{}.parquet", yesterday.format("%Y-%m-%d")));
    assert!(path.exists(), "expected {}", path.display());

    let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(&path).unwrap())
        .unwrap()
        .build()
        .unwrap();

    let mut metrics = HashSet::new();
    let mut module_v = None;
    let mut current = None;
    let mut soc = None;
    let mut power = None;
    let mut energy = None;
    let mut cell_rows = 0;
    let mut saw_battery_label = false;
    for batch in reader {
        let batch = batch.unwrap();
        let metric = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let value = batch
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let labels = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        for i in 0..batch.num_rows() {
            metrics.insert(metric.value(i).to_string());
            match metric.value(i) {
                "renogy_module_voltage_value" => module_v = Some(value.value(i)),
                "renogy_current_value" => current = Some(value.value(i)),
                "renogy_soc_percent_value" => soc = Some(value.value(i)),
                "renogy_power_watts_value" => power = Some(value.value(i)),
                "renogy_remaining_energy_wh_value" => energy = Some(value.value(i)),
                "renogy_cell_voltage_value" => cell_rows += 1,
                _ => {}
            }
            if labels.value(i).contains("SN1234") {
                saw_battery_label = true;
            }
        }
    }

    assert!(
        !metrics.contains("foo_value"),
        "foreign series leaked into the archive"
    );
    assert!((module_v.expect("module voltage") - 13.2).abs() < 1e-2);
    assert!(
        (current.expect("current") + 5.0).abs() < 1e-2,
        "signed current did not round-trip"
    );
    assert!((soc.expect("soc") - 50.0).abs() < 1e-1);
    assert!((power.expect("power") + 66.0).abs() < 1e-1);
    assert!((energy.expect("energy") - 660.0).abs() < 1.0);
    assert_eq!(cell_rows, 4, "expected one row per cell");
    assert!(
        saw_battery_label,
        "battery label missing from labels column"
    );
}
