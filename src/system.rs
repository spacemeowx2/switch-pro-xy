use anyhow::{Context, Result};
use tokio::process::Command;

pub async fn restart_bluetooth_service() -> Result<()> {
    let mut child = Command::new("systemctl")
        .arg("restart")
        .arg("bluetooth")
        .spawn()
        .context("restart bluetooth service")?;

    let status = child.wait().await?;

    if !status.success() {
        anyhow::bail!("restart bluetooth service failed");
    }

    Ok(())
}

pub async fn set_bluetooth_class(name: &str) -> Result<()> {
    let mut child = Command::new("hciconfig")
        .arg(name)
        .arg("class")
        .arg("0x02508")
        .spawn()
        .context("set bluetooth class")?;

    let status = child.wait().await?;

    if !status.success() {
        anyhow::bail!("set bluetooth class failed");
    }
    // println!("set bluetooth class success");

    Ok(())
}
