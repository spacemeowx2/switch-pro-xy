use anyhow::{Context, Result};
use bluer::{
    l2cap::{Socket, SocketAddr},
    rfcomm::{Profile, ProfileHandle, Role},
    Adapter, AdapterEvent, Address, AddressType, Session,
};
use clap::Parser;
use futures::{future::try_join, stream::StreamExt, FutureExt};
use socket::SeqPacket;
use std::{
    process::exit,
    task::Poll,
    time::{Duration, Instant},
};
use tokio::{
    io::ReadBuf,
    task::{spawn_blocking, JoinHandle},
    time::{sleep, timeout},
};
use uuid::{uuid, Uuid};

const SDP_UUID: Uuid = uuid!("00001000-0000-1000-8000-00805f9b34fb");
const SDP: &str = include_str!("./sdp/pro.xml");

mod setup;
mod socket;
mod system;
mod util;

#[derive(Parser, Debug)]
#[clap(
    name = "switch-pro-xy",
    about = "A bluetooth proxy between Switch and Pro Controller.",
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

async fn setup_pro_controller(
    _opts: &Opts,
    session: &Session,
    adapter: &Adapter,
) -> Result<ProfileHandle> {
    adapter.set_powered(true).await?;
    adapter.set_pairable(true).await?;
    adapter.set_pairable_timeout(0).await?;
    adapter.set_discoverable_timeout(180).await?;

    adapter
        .set_alias("Pro Controller".to_string())
        .await
        .context("set alias")?;
    let profile_handle = session
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

    Ok(profile_handle)
}

async fn real_main(opts: Opts) -> Result<()> {
    let controller_mac: Address = opts.controller_mac.parse().context("Controller mac")?;
    let switch_mac: Address = opts.switch_mac.parse().context("Switch mac")?;
    let session = bluer::Session::new().await.context("New Session")?;
    let adapter = session.default_adapter().await.context("Adapter")?;
    if !opts.skip_system {
        // system::hci_reset(adapter.name()).await?;
        system::restart_bluetooth_service().await?;
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
    if let Err(e) = adapter.remove_device(switch_mac).await {
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

    adapter
        .set_alias("Nintendo Switch".to_string())
        .await
        .context("set alias")?;

    let device = adapter.device(controller_mac).context("device")?;
    if let Err(e) = device.pair().await {
        println!("Pairing failed: {}", e);
    }

    println!("Controller Paired");

    let ctl_ctrl = SeqPacket::new(
        ctl_ctrl
            .connect(SocketAddr::new(controller_mac, AddressType::BrEdr, 17))
            .await
            .context("Connect ctl_ctrl")?,
    );
    let ctl_itr = SeqPacket::new(
        ctl_itr
            .connect(SocketAddr::new(controller_mac, AddressType::BrEdr, 19))
            .await
            .context("Connect ctl_itr")?,
    );

    println!("Got connection.");

    let _profile_handle = setup_pro_controller(&opts, &session, &adapter).await?;

    println!("Waiting for Switch to connect...");

    let bt_addr = adapter.address().await?;
    switch_ctrl
        .bind(SocketAddr::new(bt_addr, AddressType::BrEdr, 17))
        .context("Bind switch_ctrl")?;
    switch_itr
        .bind(SocketAddr::new(bt_addr, AddressType::BrEdr, 19))
        .context("Bind switch_itr")?;

    let switch_ctrl_listener = switch_ctrl.listen(1).context("listen switch_ctrl")?;
    let switch_itr_listener = switch_itr.listen(1).context("listen switch_itr")?;

    adapter.set_discoverable(true).await?;
    if !opts.skip_system {
        system::set_bluetooth_class(adapter.name()).await?;
    }

    let (switch_ctrl, _control_address) = switch_ctrl_listener
        .accept()
        .await
        .context("accept switch_ctrl")?;

    let (switch_itr, _interrupt_address) = switch_itr_listener
        .accept()
        .await
        .context("accept switch_itr")?;
    let switch_ctrl = SeqPacket::new(switch_ctrl);
    let switch_itr = SeqPacket::new(switch_itr);

    println!("Got Switch Connection");

    let mut buf = [0u8; 350];
    let ns_first_packet = loop {
        forward_one_packet(&ctl_itr, &switch_itr).await?;

        let ns_first_packet_len =
            match timeout(Duration::from_millis(1000), switch_itr.recv(&mut buf)).await {
                Ok(len) => len.context("recv ns first packet")?,
                Err(_) => continue,
            };

        let ns_first_packet = &buf[..ns_first_packet_len];
        println!("Got ns first packet {:x?}", ns_first_packet);
        break ns_first_packet;
    };

    ctl_itr
        .send(ns_first_packet)
        .await
        .context("send ns first packet")?;

    slow_forward(&ctl_itr, &switch_itr, controller_mac, bt_addr).await?;

    println!("About to start forwarding packets. Please close the menu in 5s");
    sleep(Duration::from_secs(5)).await;
    drain_seq_packet(&ctl_itr).await?;
    println!("Start forwarding packets");

    let ctl_task = forward_seq_packet(ctl_ctrl, switch_ctrl, false);
    let itr_task = forward_seq_packet(ctl_itr, switch_itr, true);

    try_join(ctl_task, itr_task).await?;

    Ok(())
}

async fn drain_seq_packet(socket: &SeqPacket) -> Result<()> {
    let mut buf = [0u8; 350];
    while let Some(result) = socket.recv(&mut buf).now_or_never() {
        result?;
    }
    Ok(())
}

fn replace_mac(buf: &mut [u8], find: Address, replace: Address) {
    if buf.len() < 6 {
        return;
    }
    // find mac in buf and replace to replace
    for i in 0..buf.len() - 6 {
        if buf[i..i + 6] == find.0 {
            println!("Found mac at {}", i);
            buf[i..i + 6].copy_from_slice(&replace.0);
        }
    }
}

async fn forward_one_packet(a: &SeqPacket, b: &SeqPacket) -> Result<()> {
    let mut buf = [0u8; 350];
    let len = a.recv(&mut buf).await.context("recv")?;
    let packet = &buf[..len];
    println!("Forward {:x?}", packet);
    b.send(packet).await.context("send one packet")?;
    Ok(())
}

async fn slow_forward(
    ctl: &SeqPacket,
    ns: &SeqPacket,
    find: Address,
    replace: Address,
) -> Result<()> {
    let mut buf = [0u8; 350];

    'outer: loop {
        let len = ctl.recv(&mut buf).await.context("recv")?;
        let packet = &mut buf[..len];
        replace_mac(packet, find, replace);
        println!("SLOW CTL -> NS : {:x?}", packet);
        ns.send(packet).await.context("send slow ctl packet")?;

        let mut is_set_light = false;

        if let Some(result) = ns.recv(&mut buf).now_or_never() {
            let len = result.context("recv")?;
            let packet = &buf[..len];
            println!("SLOW NS  -> CTL: {:x?}", packet);
            if packet.len() > 11 && packet[11] == 0x30 {
                is_set_light = true;
            }
            ctl.send(packet).await.context("send slow ns packet")?;
            sleep(Duration::from_millis(66)).await;

            let start = Instant::now();
            let packet = loop {
                // timed out, loop again
                if start.elapsed() > Duration::from_millis(66) {
                    continue 'outer;
                }
                let len = ctl.recv(&mut buf).await.context("recv")?;
                let packet = &mut buf[..len];
                // println!("Wait reply {:x?}", packet);
                // if it's a reply packet, send it
                if packet.len() >= 14 && packet[14] & 0x80 != 0 {
                    break packet;
                }
            };
            println!("SLOW CTL -> NS : {:x?}", packet);
            ns.send(packet)
                .await
                .context("send slow reply ctl packet")?;

            if is_set_light {
                break;
            }
        }

        // 15Hz
        sleep(Duration::from_millis(66)).await;
    }
    Ok(())
}

/// recv from a, send to b
async fn forward_seq_packet_one_way(a: SeqPacket, b: SeqPacket, low_latency: bool) -> Result<()> {
    if low_latency {
        let handle: JoinHandle<Result<()>> = spawn_blocking(move || {
            let mut buf = [0u8; 350];
            let mut read_buf = ReadBuf::new(&mut buf);

            loop {
                read_buf.clear();
                if let Poll::Ready(()) = a.poll_recv(&mut read_buf)? {
                    while let Poll::Pending = b.poll_send(read_buf.filled())? {}
                }
            }
        });

        handle.await??;

        Ok(())
    } else {
        let mut buf = [0u8; 1024];

        loop {
            let len = a
                .recv(&mut buf)
                .await
                .with_context(|| format!("one_way recv"))?;

            if len == 0 {
                continue;
            }

            b.send(&buf[..len])
                .await
                .with_context(|| format!("one_way send"))?;
        }
    }
}

async fn forward_seq_packet(a: SeqPacket, b: SeqPacket, low_latency: bool) -> Result<()> {
    if low_latency {
        util::remove_non_blocking(a.get_fd())?;
        util::remove_non_blocking(b.get_fd())?;
    }
    let a_b = forward_seq_packet_one_way(a.clone(), b.clone(), low_latency);
    let b_a = forward_seq_packet_one_way(b, a, low_latency);

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
