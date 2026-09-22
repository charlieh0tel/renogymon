use crate::error::RenogyError;
use crate::error::Result;
use crate::pdu::FunctionCode;
use crate::pdu::Pdu;
use crate::transport::Transport;
use crate::transport::TransportType;
use async_trait::async_trait;
use bluebus::AdapterProxy;
use bluebus::DeviceProxy;
use bluebus::GattCharacteristic1Proxy;
use bluebus::ObjectManagerProxy;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tokio::time::timeout;
use zbus::Connection;

const BT2_NAME_PREFIX: &str = "BT-TH-";
const BT2_WRITE_CHAR_UUID: &str = "0000ffd1-0000-1000-8000-00805f9b34fb";
const BT2_NOTIFY_CHAR_UUID: &str = "0000fff1-0000-1000-8000-00805f9b34fb";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// BT-2 Bluetooth transport for communicating with Renogy BMS devices.
pub struct Bt2Transport {
    connection: Arc<Connection>,
    write_char_path: String,
    notify_rx: mpsc::Receiver<Vec<u8>>,
    timeout: Duration,
    listener_handle: AbortHandle,
}

impl Bt2Transport {
    pub async fn connect(device_path: &str) -> Result<Self> {
        let connection = Arc::new(bluebus::get_system_connection().await?);

        let device = DeviceProxy::builder(&connection)
            .path(device_path)?
            .build()
            .await?;

        if !device.connected().await? {
            device.connect().await?;
            Self::wait_for_services(&device).await?;
        }

        let (write_char_path, notify_char_path) =
            Self::find_characteristics(&connection, device_path).await?;

        let (tx, notify_rx) = mpsc::channel(16);
        let listener_handle =
            Self::spawn_notification_listener(Arc::clone(&connection), notify_char_path, tx);

        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(Self {
            connection,
            write_char_path,
            notify_rx,
            timeout: DEFAULT_TIMEOUT,
            listener_handle,
        })
    }

    pub async fn connect_by_address(mac_address: &str, adapter: &str) -> Result<Self> {
        let mac_formatted = mac_address.replace(':', "_").to_uppercase();
        Self::connect(&format!("/org/bluez/{adapter}/dev_{mac_formatted}")).await
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    async fn wait_for_services(device: &DeviceProxy<'_>) -> Result<()> {
        for _ in 0..50 {
            if device.services_resolved().await? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(RenogyError::Bluetooth(
            "timeout waiting for services to resolve".into(),
        ))
    }

    async fn find_characteristics(
        connection: &Connection,
        device_path: &str,
    ) -> Result<(String, String)> {
        let object_manager = ObjectManagerProxy::new(connection).await?;
        let objects = object_manager.get_managed_objects().await?;

        let mut write_path = None;
        let mut notify_path = None;

        for (path, interfaces) in objects {
            let path_str = path.as_str();
            if !path_str.starts_with(device_path) {
                continue;
            }

            let Some(char_props) = interfaces.get("org.bluez.GattCharacteristic1") else {
                continue;
            };

            let Some(uuid_value) = char_props.get("UUID") else {
                continue;
            };

            let Ok(uuid) = <String as TryFrom<_>>::try_from(uuid_value.clone()) else {
                continue;
            };

            match uuid.to_lowercase().as_str() {
                BT2_WRITE_CHAR_UUID => write_path = Some(path_str.to_string()),
                BT2_NOTIFY_CHAR_UUID => notify_path = Some(path_str.to_string()),
                _ => {}
            }

            if write_path.is_some() && notify_path.is_some() {
                break;
            }
        }

        write_path
            .zip(notify_path)
            .ok_or_else(|| RenogyError::Bluetooth("BT-2 characteristics not found".into()))
    }

    fn spawn_notification_listener(
        connection: Arc<Connection>,
        notify_path: String,
        tx: mpsc::Sender<Vec<u8>>,
    ) -> AbortHandle {
        tokio::spawn(async move {
            let Ok(proxy) = GattCharacteristic1Proxy::builder(&connection)
                .destination("org.bluez")
                .and_then(|b| b.path(notify_path.as_str()))
                .map(zbus::proxy::Builder::build)
            else {
                return;
            };
            let Ok(mut char) = proxy.await else {
                return;
            };

            if char.start_notify().await.is_err() {
                return;
            }

            let mut value_changed = char.receive_value_changed().await;

            while let Some(signal) = value_changed.next().await {
                if let Ok(value) = signal.get().await
                    && let Some(data) = value.as_ref()
                    && !data.is_empty()
                {
                    let _ = tx.send(data.clone()).await;
                }
            }
        })
        .abort_handle()
    }
}

impl Drop for Bt2Transport {
    fn drop(&mut self) {
        self.listener_handle.abort();
    }
}

impl Bt2Transport {
    async fn send_pdu(&mut self, pdu: &Pdu) -> Result<Pdu> {
        let frame = pdu.serialize();

        while self.notify_rx.try_recv().is_ok() {}

        let mut write_char = GattCharacteristic1Proxy::builder(&self.connection)
            .destination("org.bluez")
            .and_then(|b| b.path(self.write_char_path.as_str()))?
            .build()
            .await?;

        write_char
            .write_value(frame, std::collections::HashMap::new())
            .await?;

        let response = timeout(self.timeout, self.notify_rx.recv())
            .await
            .map_err(|_| RenogyError::Bluetooth("timeout waiting for response".into()))?
            .ok_or_else(|| RenogyError::Bluetooth("notification channel closed".into()))?;

        Pdu::deserialize(&response)
    }
}

#[async_trait]
impl Transport for Bt2Transport {
    async fn read_holding_registers(
        &mut self,
        slave: u8,
        addr: u16,
        quantity: u16,
    ) -> Result<Vec<u16>> {
        let payload = [addr.to_be_bytes(), quantity.to_be_bytes()].concat();
        let response = self
            .send_pdu(&Pdu::new(
                slave,
                FunctionCode::ReadHoldingRegisters,
                payload,
            ))
            .await?;

        let byte_count = *response.payload.first().ok_or(RenogyError::InvalidData)? as usize;
        if response.payload.len() < 1 + byte_count {
            return Err(RenogyError::InvalidData);
        }

        Ok(response.payload[1..]
            .as_chunks::<2>()
            .0
            .iter()
            .take(quantity as usize)
            .map(|&chunk| u16::from_be_bytes(chunk))
            .collect())
    }

    async fn write_single_register(&mut self, slave: u8, addr: u16, value: u16) -> Result<()> {
        let payload = [addr.to_be_bytes(), value.to_be_bytes()].concat();
        self.send_pdu(&Pdu::new(slave, FunctionCode::WriteSingleRegister, payload))
            .await?;
        Ok(())
    }

    async fn write_multiple_registers(
        &mut self,
        slave: u8,
        addr: u16,
        values: &[u16],
    ) -> Result<()> {
        let mut payload = Vec::with_capacity(5 + values.len() * 2);
        payload.extend_from_slice(&addr.to_be_bytes());
        payload.extend_from_slice(&(values.len() as u16).to_be_bytes());
        payload.push((values.len() * 2) as u8);
        for value in values {
            payload.extend_from_slice(&value.to_be_bytes());
        }

        self.send_pdu(&Pdu::new(
            slave,
            FunctionCode::WriteMultipleRegisters,
            payload,
        ))
        .await?;
        Ok(())
    }

    async fn send_custom(&mut self, slave: u8, function_code: u8, data: &[u8]) -> Result<Vec<u8>> {
        let fc = FunctionCode::from_u8(function_code).ok_or(RenogyError::InvalidData)?;
        Ok(self
            .send_pdu(&Pdu::new(slave, fc, data.to_vec()))
            .await?
            .payload)
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Bt2
    }
}

const DEFAULT_SCAN_DURATION: Duration = Duration::from_secs(5);

pub async fn discover_bt2_devices() -> Result<Vec<bluebus::DeviceInfo>> {
    discover_bt2_devices_with_options("hci0", DEFAULT_SCAN_DURATION).await
}

async fn discover_bt2_devices_with_options(
    adapter: &str,
    scan_duration: Duration,
) -> Result<Vec<bluebus::DeviceInfo>> {
    let connection = bluebus::get_system_connection().await?;

    let adapter_path = format!("/org/bluez/{adapter}");
    let adapter_proxy = AdapterProxy::builder(&connection)
        .path(adapter_path.as_str())?
        .build()
        .await?;

    if adapter_proxy.start_discovery().await.is_ok() {
        tokio::time::sleep(scan_duration).await;
        adapter_proxy.stop_discovery().await.ok();
    }

    Ok(bluebus::list_devices()
        .await
        .into_iter()
        .filter(|d| {
            d.name
                .as_ref()
                .is_some_and(|n| n.starts_with(BT2_NAME_PREFIX))
        })
        .collect())
}
