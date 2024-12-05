use anyhow::{anyhow, Result};
use chrono::Local;
use clap::Parser;
use env_logger::Builder;
use holochain_runtime::*;
use holochain_types::prelude::*;
use log::Level;
use mr_bundle::Location;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use url2::Url2;

const SIGNAL_URL: &'static str = "wss://sbd.holo.host";
const BOOTSTRAP_URL: &'static str = "https://bootstrap-0.infra.holochain.org";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// DNA bundles for which to maintain always online nodes
    dna_bundles_paths: Vec<PathBuf>,

    /// Directory to store all holochain data
    #[arg(long)]
    data_dir: PathBuf,
}

fn wan_network_config() -> Option<WANNetworkConfig> {
    Some(WANNetworkConfig {
        signal_url: url2::url2!("{}", SIGNAL_URL),
        bootstrap_url: url2::url2!("{}", BOOTSTRAP_URL),
        ice_servers_urls: vec![
            url2::url2!("stun:stun-0.main.infra.holo.host:443"),
            url2::url2!("stun:stun-1.main.infra.holo.host:443"),
        ],
    })
}

fn log_level() -> Level {
    match std::env::var("RUST_LOG") {
        Ok(s) => Level::from_str(s.as_str()).expect("Invalid RUST_LOG level"),
        _ => Level::Info,
    }
}

fn set_wasm_level() {
    match std::env::var("WASM_LOG") {
        Ok(_s) => {}
        _ => {
            std::env::set_var("WASM_LOG", "info");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let target = Box::new(File::create(args.data_dir.join("logs.log")).expect("Can't create file"));

    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "[{}] {} - {}",
                record.level(),
                Local::now().format("%Y-%m-%dT%H:%M:%S%.3f"),
                record.args()
            )
        })
        .target(env_logger::Target::Pipe(target))
        .filter(None, log_level().to_level_filter())
        .init();
    set_wasm_level();

    let config = HolochainRuntimeConfig::new(args.data_dir.clone(), wan_network_config());

    let mut runtime = HolochainRuntime::launch(vec_to_locked(vec![])?, config).await?;
    let admin_ws = runtime.admin_websocket().await?;

    let installed_apps = admin_ws
        .list_apps(None)
        .await
        .map_err(|err| anyhow!("{err:?}"))?;

    for dna_bundle_path in args.dna_bundles_paths {
        let dna_bundle = DnaBundle::read_from_file(dna_bundle_path.as_path()).await?;

        let app_id = app_id_for_dna_bundle(&dna_bundle)?;
        let happ_bundle = wrap_dna_in_happ(dna_bundle).await?;

        if installed_apps
            .iter()
            .find(|app| app.installed_app_id.eq(&app_id))
            .is_none()
        {
            runtime
                .install_app(app_id, happ_bundle, None, None, None)
                .await?;
        }
    }

    let mut last_can_connect = can_connect_to_signal_server(url2::url2!("{}", SIGNAL_URL))
        .await
        .is_ok();
    loop {
        let can_connect = can_connect_to_signal_server(url2::url2!("{}", SIGNAL_URL))
            .await
            .is_ok();

        if last_can_connect != can_connect {
            if can_connect {
                println!("Changing from LAN only to WAN only");
            } else {
                println!("Changing from WAN only to LAN only");
            }
            last_can_connect = can_connect;
            let result = runtime.conductor_handle.shutdown().await?;
            result?;
            let config = HolochainRuntimeConfig::new(args.data_dir.clone(), wan_network_config());
            runtime = HolochainRuntime::launch(vec_to_locked(vec![])?, config).await?;
        }

        std::thread::sleep(Duration::from_secs(5));
    }
}

pub async fn can_connect_to_signal_server(signal_url: Url2) -> std::io::Result<()> {
    let config = tx5_signal::SignalConfig {
        listener: false,
        allow_plain_text: true,
        ..Default::default()
    };
    let signal_url_str = if let Some(s) = signal_url.as_str().strip_suffix('/') {
        s
    } else {
        signal_url.as_str()
    };

    tx5_signal::SignalConnection::connect(signal_url_str, Arc::new(config)).await?;

    Ok(())
}
fn app_id_for_dna_bundle(dna_bundle: &DnaBundle) -> Result<InstalledAppId> {
    let bytes = dna_bundle.encode()?;
    let hash = sha256::digest(&bytes);
    Ok(hash)
}

async fn wrap_dna_in_happ(dna_bundle: DnaBundle) -> Result<AppBundle> {
    let role_manifest = AppRoleManifest {
        name: String::from("dna"),
        provisioning: Some(CellProvisioning::Create { deferred: false }),
        dna: AppRoleDnaManifest {
            location: Some(Location::Bundled(PathBuf::from("dna.dna"))),
            modifiers: Default::default(),
            installed_hash: None,
            clone_limit: 0,
        },
    };
    let app_manifest = AppManifest::V1(AppManifestV1 {
        name: String::from(""),
        description: None,
        roles: vec![role_manifest],
        allow_deferred_memproofs: false,
    });

    let app_bundle = AppBundle::new(
        app_manifest,
        vec![(PathBuf::from("dna.dna"), dna_bundle)],
        PathBuf::from(""),
    )
    .await?;
    Ok(app_bundle)
}
