use clap::Parser;
use clap::Subcommand;
use prometheus_client::registry::Registry;
use renogy::any_transport::AnyTransport;
use renogy::any_transport::BT2_SCAN_RANGE;
use renogy::any_transport::SERIAL_SCAN_RANGE;
use renogy::bt2::Bt2Transport;
use renogy::bt2::discover_bt2_devices;
use renogy::collector::buffer::SampleBuffer;
use renogy::collector::energy::EnergyTracker;
use renogy::collector::metrics::PrometheusMetrics;
use renogy::collector::server::MetricsServer;
use renogy::collector::writer::VmWriter;
use renogy::serial::SerialTransport;
use renogy::util::parse_address;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

/// Intervals longer than this many polls are not integrated into energy.
const MAX_GAP_POLLS: u32 = 4;

#[derive(Parser)]
#[command(name = "renogymon-bms-collector")]
#[command(about = "Collect Renogy BMS metrics and export to VictoriaMetrics")]
struct Args {
    #[command(subcommand)]
    transport: TransportCmd,

    /// Polling interval in seconds (minimum 1)
    #[arg(long, default_value_t = 15)]
    poll_interval: u64,

    /// VictoriaMetrics URL
    #[arg(long, default_value = "http://localhost:8428")]
    vm_url: String,

    /// How long to buffer samples when VM is unavailable (minutes)
    #[arg(long, default_value_t = 15)]
    buffer_duration: u64,

    /// Port for /metrics endpoint
    #[arg(long, default_value_t = 9090)]
    metrics_port: u16,

    /// Disable push to VictoriaMetrics (pull only via /metrics)
    #[arg(long)]
    disable_push: bool,

    /// Disable /metrics endpoint (push only)
    #[arg(long)]
    disable_pull: bool,
}

#[derive(Subcommand)]
enum TransportCmd {
    /// Connect via BT-2 Bluetooth adapter
    Bt2 {
        /// BT-2 MAC address
        #[arg(short, long)]
        mac: Option<String>,

        /// Bluetooth adapter name
        #[arg(short, long, default_value = "hci0")]
        adapter: String,

        /// BMS addresses to monitor
        #[arg(short = 'b', long, value_parser = parse_address)]
        bms_addresses: Vec<u8>,
    },
    /// Connect via serial/RS-485
    Serial {
        /// Serial port path
        #[arg(short, long)]
        port: String,

        /// Baud rate
        #[arg(short = 'r', long, default_value_t = 9600)]
        baud_rate: u32,

        /// BMS addresses to monitor
        #[arg(short, long, value_parser = parse_address)]
        bms_addresses: Vec<u8>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let poll_interval = Duration::from_secs(args.poll_interval.max(1));
    let buffer_duration = Duration::from_secs(args.buffer_duration * 60);

    let cancel = CancellationToken::new();

    let cancel_signal = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Received shutdown signal");
        cancel_signal.cancel();
    });

    let (mut transport, addresses) = match args.transport {
        TransportCmd::Bt2 {
            mac,
            adapter,
            bms_addresses,
        } => {
            let mac_address = if let Some(mac) = mac {
                mac
            } else {
                tracing::info!("Discovering BT-2 devices...");
                let devices = discover_bt2_devices().await?;
                if devices.is_empty() {
                    return Err("No BT-2 devices found. Specify a MAC address with --mac".into());
                }
                for device in &devices {
                    tracing::info!(
                        "Found: {} ({})",
                        device.name.as_deref().unwrap_or("unknown"),
                        device.address
                    );
                }
                devices[0].address.clone()
            };

            tracing::info!("Connecting to {} via {}...", mac_address, adapter);
            let mut transport: AnyTransport =
                Bt2Transport::connect_by_address(&mac_address, &adapter)
                    .await?
                    .into();

            let addresses = if bms_addresses.is_empty() {
                tracing::info!("Scanning for batteries at {:02X?}...", BT2_SCAN_RANGE);
                let found = transport.discover_batteries(BT2_SCAN_RANGE).await;
                tracing::info!("Found {} battery(s)", found.len());
                found
            } else {
                bms_addresses
            };

            (transport, addresses)
        }
        TransportCmd::Serial {
            port,
            baud_rate,
            bms_addresses,
        } => {
            tracing::info!("Opening {} at {} baud...", port, baud_rate);
            let first_addr = bms_addresses.first().copied().unwrap_or(0x01);
            let mut transport: AnyTransport = SerialTransport::new(&port, baud_rate, first_addr)
                .await?
                .into();

            let addresses = if bms_addresses.is_empty() {
                tracing::info!("Scanning for batteries at {:02X?}...", SERIAL_SCAN_RANGE);
                let found = transport.discover_batteries(SERIAL_SCAN_RANGE).await;
                tracing::info!("Found {} battery(s)", found.len());
                found
            } else {
                bms_addresses
            };

            (transport, addresses)
        }
    };

    if addresses.is_empty() {
        return Err("No batteries found!".into());
    }

    tracing::info!(
        "Monitoring {} battery(s) at addresses: {:02X?}",
        addresses.len(),
        addresses
    );

    let metrics = Arc::new(PrometheusMetrics::default());
    let mut registry = Registry::default();
    metrics.register(&mut registry);
    let registry = Arc::new(registry);

    let max_samples = (buffer_duration.as_secs() / poll_interval.as_secs().max(1)) as usize;
    let buffer = SampleBuffer::new(max_samples);

    let mut handles = Vec::new();

    if !args.disable_pull {
        let server = MetricsServer::new(registry.clone(), args.metrics_port, cancel.clone());
        handles.push(tokio::spawn(async move {
            if let Err(e) = server.run().await {
                tracing::error!("Metrics server error: {}", e);
            }
        }));
    }

    if !args.disable_push {
        let writer = VmWriter::new(&args.vm_url, buffer.clone(), cancel.clone());
        handles.push(tokio::spawn(async move {
            writer.run().await;
        }));
    }

    run_poller(
        &mut transport,
        &addresses,
        poll_interval,
        &metrics,
        &buffer,
        cancel.clone(),
    )
    .await;

    for handle in handles {
        handle.await.ok();
    }

    tracing::info!("Shutdown complete");
    Ok(())
}

async fn run_poller(
    transport: &mut AnyTransport,
    addresses: &[u8],
    poll_interval: Duration,
    metrics: &PrometheusMetrics,
    buffer: &SampleBuffer,
    cancel: CancellationToken,
) {
    let mut interval = tokio::time::interval(poll_interval);
    let mut energy = EnergyTracker::new(poll_interval * MAX_GAP_POLLS);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = cancel.cancelled() => {
                tracing::info!("Poller stopping");
                return;
            }
        }

        for &addr in addresses {
            match transport.query_battery(addr).await {
                Some(mut info) => {
                    info.serial = format!("{}_{:02X}", info.serial, addr);
                    tracing::debug!(
                        "Battery 0x{:02X}: {:.1}V {:.1}A {:.1}%",
                        addr,
                        info.module_voltage,
                        info.current,
                        info.soc_percent
                    );
                    metrics.update(&info);
                    buffer.push(energy.observe(info));
                }
                None => {
                    tracing::warn!("Failed to query battery at 0x{:02X}", addr);
                }
            }
        }
    }
}
