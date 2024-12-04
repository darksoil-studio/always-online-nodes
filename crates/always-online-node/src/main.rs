use anyhow::{anyhow, Result};
use clap::Parser;
use holochain_runtime::*;
use holochain_types::prelude::*;
use mr_bundle::Location;
use std::path::PathBuf;

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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let config = HolochainRuntimeConfig::new(args.data_dir, wan_network_config());

    let runtime = HolochainRuntime::launch(vec_to_locked(vec![])?, config).await?;
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

    // Just be online always
    loop {
        std::thread::park();
    }
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
