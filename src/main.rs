use bluer::adv::Advertisement;
use clap::Parser;
use std::process::exit;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[clap(
    name = "switch-pro-xy",
    about = "A bluetooth proxy between Switch and Pro Controller. ",
    author = "spacemeowx2",
    version = env!("CARGO_PKG_VERSION"),
)]
struct Opts {}

async fn real_main(opts: Opts) -> Result<()> {
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;

    adapter.set_powered(true).await?;

    adapter.set_discoverable_timeout(0).await?;
    adapter.set_discoverable(true).await?;

    let le_advertisement = Advertisement {
        discoverable: Some(true),
        ..Default::default()
    };

    adapter.advertise(le_advertisement).await?;

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    env_logger::init();
    let opts: Opts = Opts::parse();
    let result = real_main(opts).await;

    match result {
        Ok(_) => exit(0),
        Err(err) => {
            eprintln!("Error: {}", &err);
            exit(2);
        }
    }
}
