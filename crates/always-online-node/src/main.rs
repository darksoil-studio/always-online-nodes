use anyhow::{anyhow, Result};
use chrono::Local;
use clap::Parser;
use env_logger::Builder;
use holochain_client::ZomeCallTarget;
use holochain_conductor_api::CellInfo;
use holochain_runtime::*;
use holochain_types::prelude::*;
use log::Level;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use url2::Url2;

const SIGNAL_URL: &'static str = "wss://sbd.holo.host";
const BOOTSTRAP_URL: &'static str = "https://bootstrap.holo.host";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// hApp bundles for which to maintain always online nodes
    happ_bundles_paths: Vec<PathBuf>,

    /// Directory to store all holochain data
    #[arg(long)]
    data_dir: PathBuf,

    /// Directory to store all holochain data
    #[arg(long)]
    lan_only: bool,
}

fn wan_network_config() -> Option<WANNetworkConfig> {
    Some(WANNetworkConfig {
        signal_url: url2::url2!("{}", SIGNAL_URL),
        bootstrap_url: url2::url2!("{}", BOOTSTRAP_URL),
        ice_servers_urls: vec![],
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

    let data_dir = args.data_dir;
    if data_dir.exists() {
        if !std::fs::read_dir(&data_dir).is_ok() {
            return Err(anyhow!("The given data dir is not a directory."));
        };
    } else {
        std::fs::create_dir_all(data_dir.clone())?;
    }

    let target = Box::new(File::create(data_dir.join("logs.log")).expect("Can't create file"));

    Builder::new()
        .format(|buf, record| {
            if record.args().to_string().contains("spawn_") {
                Ok(())
            } else {
                writeln!(
                    buf,
                    "[{}] {} - {}",
                    record.level(),
                    Local::now().format("%Y-%m-%dT%H:%M:%S%.3f"),
                    record.args()
                )
            }
        })
        .target(env_logger::Target::Pipe(target))
        .filter(None, log_level().to_level_filter())
        .filter_module("holochain_sqlite", log::LevelFilter::Off)
        .init();
    set_wasm_level();

    let wan_config = match args.lan_only {
        true => None,
        false => wan_network_config(),
    };

    let config = HolochainRuntimeConfig::new(data_dir.clone(), wan_config);

    let mut runtime = HolochainRuntime::launch(vec_to_locked(vec![])?, config).await?;
    let admin_ws = runtime.admin_websocket().await?;

    let installed_apps = admin_ws
        .list_apps(None)
        .await
        .map_err(|err| anyhow!("{err:?}"))?;

    let mut app_ids: Vec<String> = installed_apps
        .iter()
        .map(|app| app.installed_app_id.clone())
        .collect();

    for happ_bundle_path in args.happ_bundles_paths {
        let happ_bundle = read_from_file(&happ_bundle_path).await?;

        let app_id = happ_bundle.manifest().app_name().to_string();

        if installed_apps
            .iter()
            .find(|app| app.installed_app_id.eq(&app_id))
            .is_none()
        {
            let app_info = runtime
                .install_app(app_id.clone(), happ_bundle, None, None, None)
                .await?;
            let app_ws = runtime
                .app_websocket(
                    app_id.clone(),
                    holochain_types::websocket::AllowedOrigins::Any,
                )
                .await?;

            for (_role, cell_infos) in app_info.cell_info {
                for cell_info in cell_infos {
                    let Some(cell_id) = cell_id(&cell_info) else {
                        continue;
                    };
                    let dna_def = admin_ws
                        .get_dna_definition(cell_id.dna_hash().clone())
                        .await
                        .map_err(|err| anyhow!("{err:?}"))?;

                    let Some(first_zome) = dna_def.coordinator_zomes.first() else {
                        continue;
                    };

                    app_ws
                        .call_zome(
                            ZomeCallTarget::CellId(cell_id),
                            first_zome.0.clone(),
                            "init".into(),
                            ExternIO::encode(())?,
                        )
                        .await
                        .map_err(|err| anyhow!("{:?}", err))?;
                }
            }

            app_ids.push(app_id.clone());

            log::info!("Installed app for hApp {}", app_id);
        }
    }

    log::info!("Starting always online node for DNAs {:?}", app_ids);

    let mut last_can_connect = can_connect_to_signal_server(url2::url2!("{}", SIGNAL_URL))
        .await
        .is_ok();

    let mut last_boot_time = std::time::SystemTime::now();

    loop {
        let can_connect = can_connect_to_signal_server(url2::url2!("{}", SIGNAL_URL))
            .await
            .is_ok();

        if last_can_connect != can_connect {
            if can_connect {
                log::warn!("Changing from LAN only to WAN only");
            } else {
                log::warn!("Changing from WAN only to LAN only");
            }
            last_can_connect = can_connect;
            let result = runtime.conductor_handle.shutdown().await?;
            result?;
            let wan_config = match args.lan_only {
                true => None,
                false => wan_network_config(),
            };
            let config = HolochainRuntimeConfig::new(data_dir.clone(), wan_config);
            runtime = HolochainRuntime::launch(vec_to_locked(vec![])?, config).await?;
        }

        // Reboot every 30 mins
        let now = std::time::SystemTime::now();
        if last_boot_time.elapsed()? > Duration::from_secs(30 * 60) {
            log::info!("Performing scheduled reboot every 30 mins.");

            last_boot_time = now;

            let result = runtime.conductor_handle.shutdown().await?;
            result?;
            let wan_config = match args.lan_only {
                true => None,
                false => wan_network_config(),
            };
            let config = HolochainRuntimeConfig::new(data_dir.clone(), wan_config);
            runtime = HolochainRuntime::launch(vec_to_locked(vec![])?, config).await?;
        }

        std::thread::sleep(Duration::from_secs(30));
    }
}

async fn read_from_file(happ_bundle_path: &PathBuf) -> Result<AppBundle> {
    mr_bundle::Bundle::read_from_file(happ_bundle_path)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

fn cell_id(cell_info: &CellInfo) -> Option<CellId> {
    match cell_info {
        CellInfo::Provisioned(provisioned) => Some(provisioned.cell_id.clone()),
        CellInfo::Cloned(cloned) => Some(cloned.cell_id.clone()),
        CellInfo::Stem(_) => None,
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
