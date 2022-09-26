use anyhow::{Context, Result};
use bluer::{
    l2cap::{SeqPacket, Socket, SocketAddr},
    rfcomm::{Profile, Role},
    Adapter, AdapterEvent, Address, AddressType, Session,
};
use clap::Parser;
use futures::{future::try_join, stream::StreamExt};
use std::{process::exit, time::Duration};
use tokio::{task::JoinHandle, time::sleep};
use uuid::{uuid, Uuid};

const SDP_UUID: Uuid = uuid!("00001000-0000-1000-8000-00805f9b34fb");
const SDP: &str = include_str!("./sdp/pro.xml");

mod setup;
mod system;

#[derive(Parser, Debug)]
#[clap(
    name = "switch-pro-xy",
    about = "A bluetooth proxy between Switch and Pro Controller. ",
    author = "spacemeowx2",
    version = env!("CARGO_PKG_VERSION"),
)]
struct Opts {
    #[clap(long)]
    skip_bluez_setup: bool,
    #[clap(long)]
    skip_system: bool,
    #[clap(value_parser)]
    controller_mac: String,
    #[clap(value_parser)]
    switch_mac: String,
}

async fn setup_pro_controller(opts: &Opts, session: &Session, adapter: &Adapter) -> Result<()> {
    adapter.set_powered(true).await?;
    adapter.set_pairable(true).await?;
    adapter.set_pairable_timeout(0).await?;
    adapter.set_discoverable_timeout(180).await?;

    adapter
        .set_alias("Pro Controller".to_string())
        .await
        .context("set alias")?;
    session
        .register_profile(Profile {
            uuid: SDP_UUID,
            service_record: Some(SDP.to_string()),
            role: Some(Role::Server),
            require_authentication: Some(false),
            require_authorization: Some(false),
            auto_connect: Some(true),

            ..Default::default()
        })
        .await
        .context("register profile")?;

    Ok(())
}

async fn real_main(opts: Opts) -> Result<()> {
    let controller_mac: Address = opts.controller_mac.parse().context("Controller mac")?;
    let switch_mac: Address = opts.switch_mac.parse().context("Switch mac")?;
    let session = bluer::Session::new().await.context("New Session")?;
    let adapter = session.default_adapter().await.context("Adapter")?;
    if !opts.skip_system {
        system::hci_reset(adapter.name()).await?;
    }
    adapter.set_powered(true).await?;

    if !opts.skip_bluez_setup {
        setup::setup_bluez().await?;
    }

    let ctl_ctrl = Socket::new_seq_packet()?;
    let ctl_itr = Socket::new_seq_packet()?;

    let switch_ctrl = Socket::new_seq_packet()?;
    let switch_itr = Socket::new_seq_packet()?;

    if let Err(e) = adapter.remove_device(controller_mac).await {
        println!("Failed to unpair: {:?}", e);
    }

    println!("Connecting to Pro Controller {}", controller_mac);

    let mut stream = adapter
        .discover_devices()
        .await
        .context("discover devices")?;

    while let Some(device) = stream.next().await {
        if let AdapterEvent::DeviceAdded(addr) = device {
            if addr == controller_mac {
                println!("Pro Controller found");
                break;
            }
        }
    }

    drop(stream);

    let device = adapter.device(controller_mac).context("device")?;
    if let Err(e) = device.pair().await {
        println!("Pairing failed: {}", e);
    }

    println!("Controller Paired");

    adapter
        .set_alias("Nintendo Switch".to_string())
        .await
        .context("set alias")?;

    let ctl_ctrl = ctl_ctrl
        .connect(SocketAddr::new(controller_mac, AddressType::BrEdr, 17))
        .await
        .context("Connect ctl_ctrl")?;
    let ctl_itr = ctl_itr
        .connect(SocketAddr::new(controller_mac, AddressType::BrEdr, 19))
        .await
        .context("Connect ctl_itr")?;

    println!("Got connection.");

    setup_pro_controller(&opts, &session, &adapter).await?;

    println!("Waiting for Switch to connect...");

    let bt_addr = adapter.address().await?;
    switch_ctrl
        .bind(SocketAddr::new(bt_addr, AddressType::BrEdr, 17))
        .context("Bind switch_ctrl")?;
    switch_itr
        .bind(SocketAddr::new(bt_addr, AddressType::BrEdr, 19))
        .context("Bind switch_itr")?;

    let task_adapter = adapter.clone();
    let unpair_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(10)).await;
            if let Err(e) = task_adapter.remove_device(switch_mac).await {
                println!("Failed to unpair: {:?}", e);
            }
        }
    });

    let switch_ctrl_listener = switch_ctrl.listen(1).context("listen switch_ctrl")?;
    let switch_itr_listener = switch_itr.listen(1).context("listen switch_itr")?;

    adapter.set_discoverable(true).await?;
    if !opts.skip_system {
        system::set_bluetooth_class(adapter.name()).await?;
    }

    let (switch_ctrl, control_address) = switch_ctrl_listener
        .accept()
        .await
        .context("accept switch_itr")?;
    println!("Got Switch Control Client Connection");

    let (switch_itr, interrupt_address) = switch_itr_listener
        .accept()
        .await
        .context("accept switch_ctrl")?;
    println!("Got Switch Interrupt Client Connection");

    unpair_task.abort();

    let ctl_task = bridge_seq_packet("ctrl", &ctl_ctrl, &switch_ctrl);
    let itr_task = bridge_seq_packet("itr ", &ctl_itr, &switch_itr);

    try_join(ctl_task, itr_task).await?;

    Ok(())
}

async fn bridge_seq_packet(name: &str, a: &SeqPacket, b: &SeqPacket) -> Result<()> {
    let a_b = async {
        let mut buf = [0u8; 1024];
        loop {
            let n = a.recv(&mut buf).await?;
            println!("{} A -> B: {} {:x?}", name, n, &buf[..n]);

            if n == 0 {
                break;
            }

            b.send(&buf[..n]).await?;
        }
        Ok::<(), anyhow::Error>(())
    };
    let b_a = async {
        let mut buf = [0u8; 1024];
        loop {
            let n = b.recv(&mut buf).await?;
            println!("{} B -> A: {} {:x?}", name, n, &buf[..n]);

            if n == 0 {
                break;
            }

            a.send(&buf[..n]).await?;
        }
        Ok::<(), anyhow::Error>(())
    };

    try_join(a_b, b_a).await?;

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    env_logger::init();
    let opts: Opts = Opts::parse();
    println!("{:?}", opts);
    let result = real_main(opts).await;

    match result {
        Ok(_) => exit(0),
        Err(err) => {
            eprintln!("Error: {:?}", &err);
            exit(2);
        }
    }
}
