use anyhow::Result;
use tokio::{fs::File, io::AsyncReadExt, io::AsyncWriteExt};

pub async fn save_file(filename: &str, file_content: &[u8]) -> Result<()> {
    let f = File::create(filename).await?;
    let mut writer = tokio::io::BufWriter::new(f);
    writer.write_all(file_content).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_file(filename: &str) -> Result<Vec<u8>> {
    let f = File::open(filename).await?;
    let mut r = tokio::io::BufReader::new(f);
    let mut content = Vec::new();
    r.read_to_end(&mut content).await?;
    Ok(content)
}
