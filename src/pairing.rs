use async_trait::async_trait;
use bluest::{
    pairing::{IoCapability, PairingAgent, PairingRejected, Passkey},
    Device,
};
pub(crate) struct StdioPairingAgent;

#[async_trait]
impl PairingAgent for StdioPairingAgent {
    /// The input/output capabilities of this agent
    fn io_capability(&self) -> IoCapability {
        IoCapability::KeyboardDisplay
    }

    async fn confirm(&self, device: &Device) -> Result<(), PairingRejected> {
        tokio::task::block_in_place(move || {
            println!(
                "Do you want to pair with {:?}? (Y/n)",
                device.name().unwrap()
            );
            let mut buf = String::new();
            std::io::stdin()
                .read_line(&mut buf)
                .map_err(|_| PairingRejected::default())?;
            let response = buf.trim();
            if response.is_empty() || response == "y" || response == "Y" {
                Ok(())
            } else {
                Err(PairingRejected::default())
            }
        })
    }

    async fn confirm_passkey(
        &self,
        device: &Device,
        passkey: Passkey,
    ) -> Result<(), PairingRejected> {
        tokio::task::block_in_place(move || {
            println!(
                "Is the passkey \"{}\" displayed on {:?}? (Y/n)",
                passkey,
                device.name().unwrap()
            );
            let mut buf = String::new();
            std::io::stdin()
                .read_line(&mut buf)
                .map_err(|_| PairingRejected::default())?;
            let response = buf.trim();
            if response.is_empty() || response == "y" || response == "Y" {
                Ok(())
            } else {
                Err(PairingRejected::default())
            }
        })
    }

    async fn request_passkey(&self, device: &Device) -> Result<Passkey, PairingRejected> {
        tokio::task::block_in_place(move || {
            println!(
                "Please enter the 6-digit passkey for {:?}: ",
                device.name().unwrap()
            );
            let mut buf = String::new();
            std::io::stdin()
                .read_line(&mut buf)
                .map_err(|_| PairingRejected::default())?;
            buf.trim().parse().map_err(|_| PairingRejected::default())
        })
    }

    fn display_passkey(&self, device: &Device, passkey: Passkey) {
        println!(
            "The passkey is \"{}\" for {:?}.",
            passkey,
            device.name().unwrap()
        );
    }
}
