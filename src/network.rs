use super::pairing::StdioPairingAgent;
use super::Command;
use anyhow::Result;
use bluest::{Adapter, Characteristic, Device, Uuid};
use futures_util::{AsyncBufReadExt, StreamExt, TryStreamExt};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{Mutex, Notify};

const NORDIC_UUID: &str = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
const NORDIC_UART_TX_UUID: &str = "6e400002-b5a3-f393-e0a9-e50e24dcca9e";
const NORDIC_UART_RX_UUID: &str = "6e400003-b5a3-f393-e0a9-e50e24dcca9e";

const END_TOKEN: &str = "8210409291035902";
const END_CHECKSUM: &str = "8210409291035902\r\n>";

pub struct Communicator {
    adapter: Adapter,
    rx: Characteristic,
    tx: Characteristic,
    bangle: Device,
    paused_notifier: Notify,
    receive_notifier: Notify,
    paused: AtomicBool,
    pub command: Mutex<Option<Command>>,
}

impl Communicator {
    pub async fn new() -> Result<Self> {
        // open bluetooth
        let adapter = Adapter::default()
            .await
            .ok_or_else(|| anyhow::anyhow!("Bluetooth adapter not found"))?;
        adapter.wait_available().await?;

        // find the watch
        let bangle = find_banglejs(&adapter).await?;

        // get the communication channels from the watch
        let (tx, rx) = tx_rx(&bangle).await?;

        Ok(Communicator {
            adapter,
            rx,
            tx,
            bangle,
            paused_notifier: Notify::new(),
            receive_notifier: Notify::new(),
            paused: AtomicBool::new(false),
            command: Mutex::new(None),
        })
    }

    pub async fn disconnect(&self) -> Result<()> {
        println!("disconnecting");
        self.adapter.disconnect_device(&self.bangle).await?;
        Ok(())
    }

    pub async fn send_message(&self, msg: &str) -> Result<()> {
        let msg = format!("{}\n\x10console.log('{}');\n", msg, END_TOKEN);
        for chunk in msg.as_bytes().chunks(16) {
            while self.paused.load(Ordering::Relaxed) {
                self.paused_notifier.notified().await;
            }
            self.tx.write(chunk).await?;
        }
        self.receive_notifier.notified().await;
        Ok(())
    }
}

pub async fn receive_messages(comms: Arc<Communicator>) -> Result<()> {
    let (rx, command, receive_notifier) = (&comms.rx, &comms.command, &comms.receive_notifier);
    let msgs = rx.notify().await?;
    msgs.map_ok(|mut v| {
        // pause or restart comms if we receive characters 17 or 19
        let mut pause_change = None;
        v.retain(|c| {
            if *c == 17 {
                // XON: resume upload
                pause_change = Some(false);
                false
            } else if *c == 19 {
                // XOFF: stop upload
                pause_change = Some(true);
                false
            } else {
                true
            }
        });
        if let Some(pause) = pause_change {
            comms.paused.store(pause, Ordering::Relaxed);
            if !pause {
                comms.paused_notifier.notify_one()
            }
        }
        v
    })
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    .into_async_read()
    .lines()
    .try_filter(|line| {
        // hide all lines starting with char 10
        futures_util::future::ready(if line.starts_with('\x10') || line == END_TOKEN {
            true
        } else {
            println!("{line}");
            false
        })
    })
    .try_fold(String::new(), |mut full_message, line| async move {
        if line == END_TOKEN {
            let current_command = command.lock().await.clone();
            if let Some(Command::Get { filename: f }) = current_command {
                todo!()
            }
            receive_notifier.notify_one();
            full_message.clear();
        } else {
            full_message.push_str(&line);
        }
        Ok(full_message)
    })
    .await?;
    Ok(())
}

async fn find_banglejs(adapter: &Adapter) -> Result<Device> {
    let nordic_uuid = Uuid::parse_str(NORDIC_UUID)?;
    let mut connected_devices = adapter
        .connected_devices_with_services(&[nordic_uuid])
        .await?;
    if let Some(device) = connected_devices.pop() {
        println!("we are already connected");
        return Ok(device);
    }
    println!("starting scan");
    let mut scan = adapter.scan(&[]).await?;
    println!("scan started");
    while let Some(discovered_device) = scan.next().await {
        if discovered_device.adv_data.services.contains(&nordic_uuid) {
            println!("we found it !");
            let device = discovered_device.device;

            println!("connecting");
            adapter.connect_device(&device).await?;
            println!("connected");
            while !device.is_paired().await? {
                println!("we are not paired yet, trying pairing");
                let mut l = String::new();
                std::io::stdin().read_line(&mut l)?;
                device.pair_with_agent(&StdioPairingAgent).await?;
            }
            println!("we are paired");
            return Ok(device);
        }
    }
    anyhow::bail!("no banglejs device found")
}

async fn tx_rx(bangle: &Device) -> Result<(Characteristic, Characteristic)> {
    let nordic_uuid = Uuid::parse_str(NORDIC_UUID)?;
    let nordic_tx_uuid = Uuid::parse_str(NORDIC_UART_TX_UUID)?;
    let nordic_rx_uuid = Uuid::parse_str(NORDIC_UART_RX_UUID)?;

    let services = bangle.discover_services_with_uuid(nordic_uuid).await?;
    let service = services
        .into_iter()
        .find(|s| s.uuid() == nordic_uuid)
        .ok_or_else(|| anyhow::anyhow!("no nordic service"))?;
    let tx = service
        .discover_characteristics_with_uuid(nordic_tx_uuid)
        .await?
        .into_iter()
        .find(|c| c.uuid() == nordic_tx_uuid)
        .ok_or_else(|| anyhow::anyhow!("no tx"))?;
    let rx = service
        .discover_characteristics_with_uuid(nordic_rx_uuid)
        .await?
        .into_iter()
        .find(|c| c.uuid() == nordic_rx_uuid)
        .ok_or_else(|| anyhow::anyhow!("no rx"))?;
    Ok((tx, rx))
}
