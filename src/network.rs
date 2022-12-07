use super::pairing::StdioPairingAgent;
use super::Command;
use anyhow::Result;
use bluest::{Adapter, Characteristic, Device, Uuid};
use futures_util::StreamExt;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::{
    io::AsyncWriteExt,
    sync::{Mutex, Notify},
};

const NORDIC_UUID: &str = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
const NORDIC_UART_TX_UUID: &str = "6e400002-b5a3-f393-e0a9-e50e24dcca9e";
const NORDIC_UART_RX_UUID: &str = "6e400003-b5a3-f393-e0a9-e50e24dcca9e";

const END_TOKEN: &str = "8210409291035902";
const END_CHECKSUM: &str = "8210409291035902\r\n=undefined\r\n>";

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
        let msg = format!("{}\n;console.log('{}');\n", msg, END_TOKEN);
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
    let mut msgs = comms.rx.notify().await?;
    let mut full_message = Vec::new();
    while let Some(msg) = msgs.next().await {
        let mut msg = msg?;
        let mut pause_change = None;
        msg.retain(|c| {
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
        full_message.extend(msg.iter().copied());
        if full_message.ends_with(END_CHECKSUM.as_bytes()) {
            let command = { comms.command.lock().await.take() };
            match command {
                Some(Command::Ls) => {
                    if let Ok(decoded_msg) = std::str::from_utf8(&full_message) {
                        let lines: Vec<&str> = decoded_msg.lines().skip(1).collect();
                        lines
                            .iter()
                            .take(lines.len() - 5)
                            .for_each(|l| println!("{}", l));
                    }
                }
                Some(Command::SyncClock) => (),
                Some(Command::Download { filename: f }) => {
                    if let Ok(decoded_msg) = std::str::from_utf8(&full_message) {
                        let lines: Vec<&str> = decoded_msg.lines().skip(1).collect();
                        let bytes: Vec<u8> = lines
                            .iter()
                            .take(lines.len() - 5)
                            .map(|l| l.parse::<u8>().unwrap())
                            .collect();
                        save_file(&f, &bytes).await?;
                    }
                }
                Some(Command::Rm { filename: _ }) => (),
                None => panic!("there was nothing to receive"),
                _ => todo!(),
            }
            comms.receive_notifier.notify_one();
            full_message.clear();
        }
    }
    Ok(())
}

async fn save_file(filename: &str, file_content: &[u8]) -> Result<()> {
    let f = tokio::fs::File::create(filename).await?;
    let mut writer = tokio::io::BufWriter::new(f);
    writer.write_all(file_content).await?;
    writer.flush().await?;
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
