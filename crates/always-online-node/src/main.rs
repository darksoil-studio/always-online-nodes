use anyhow::Result;
use clap::{Args, Parser};
use holochain_runtime::*;
use holochain_types::prelude::*;
use mr_bundle::Location;

const SIGNAL_URL: &'static str = "wss://sbd.holo.host";
const BOOTSTRAP_URL: &'static str = "https://bootstrap-0.infra.holochain.org";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// DNA bundles for which to maintain always online nodes
    dna_bundles_paths: Vec<PathBuf>,

    /// Directory to store all holochain data
    #[arg(long)]
    holochain_dir: PathBuf,
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

#[tokio::async_main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let config = HolochainRuntimeConfig::new(args.holochain_dir, wan_network_config());

    let runtime = HolochainRuntime::launch(vec_to_locked(vec![]), config).await?;
    let admin_ws = runtime.admin_ws().await?;

    let installed_apps = admin_ws.installed_apps(None).await?;

    for dna_bundle_path in args.dna_bundles_path {
        let dna_bundle = DnaBundle::read_from_file(dna_bundle_path)?;

        let app_id = app_id_for_dna_bundle(dna_bundle.clone())?;
        let happ_bundle = wrap_dna_in_happ(dna_bundle)?;

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

fn app_id_for_dna_bundle(dna_bundle: DnaBundle) -> Result<InstalledAppId> {
    let bytes = dna_bundle.encode()?;
    let hash = sha256::digest(&bytes);
    Ok(hash)
}

fn wrap_dna_in_happ(dna_bundle: DnaBundle) -> Result<AppBundle> {
    let role_manifest = AppRoleManifest {
        name: String::from("dna"),
        provisioning: Some(CellProvisioning::Create { deferred: false }),
        dna: AppRoleDnaManifest {
            location: Some(Location::Bundled()),
            modifiers: None,
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
    );
    Ok(app_bundle)
}
