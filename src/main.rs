/*!
 * Creates a DIDComm agent for each Ollama model allowing for private and secure chat interface.
 *
 */

use std::{collections::HashMap, process, sync::Arc};

use affinidi_messaging_sdk::{ATM, config::ATMConfig, profiles::ATMProfile};
use affinidi_tdk::{
    common::{TDKSharedState, environments::TDKEnvironments},
    secrets_resolver::SecretsResolver,
};
use anyhow::Result;
use clap::Parser;
use console::style;
use didcomm_ai_bridge::{
    activate::get_secrets,
    agents::{
        concierge::concierge_handler::{Concierge, ConciergeMessage},
        state_management::SharedState,
    },
    termination::{Interrupted, create_termination},
};
use setup_wizard::run_setup_wizard;
use std::env;
use tokio::{sync::mpsc, try_join};
use tracing::info;
use tracing_subscriber::filter;

mod setup_wizard;

/// DIDComm agent for your Ollama models
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Setup Wizard (Runs automatically if no models are configured)
    #[arg(short, long)]
    setup_wizard: bool,

    /// Add an Ollama model to the DIDComm agent
    #[arg(short, long)]
    add_model: bool,

    #[arg(short, long)]
    /// Alternative configuration file
    config_file: Option<String>,

    /// Environment to use
    #[arg(short, long)]
    environment: Option<String>,

    /// Path to the environments file (defaults to environments.json)
    #[arg(short, long)]
    path_environments: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // construct a subscriber that prints formatted traces to stdout
    let subscriber = tracing_subscriber::fmt()
        // Use a more compact, abbreviated log format
        .with_env_filter(filter::EnvFilter::from_default_env())
        .finish();
    // use that subscriber to process traces emitted after this point
    tracing::subscriber::set_global_default(subscriber).expect("Logging failed, exiting...");

    let config_file = if let Some(config_file) = args.config_file {
        config_file
    } else {
        "config.json".to_string()
    };

    let config = match SharedState::load(&config_file) {
        Ok(config) => Arc::new(config),
        Err(e) => {
            if e.to_string()
                .starts_with("Couldn't open configuration file")
            {
                println!("{}", style("ERROR: No configuration file found.").red());
                let config = run_setup_wizard().await?;
                config.save(&config_file).await?;
                println!("New config created, please update it, if needed, and re-run the app");
                process::exit(0);
            } else {
                let root_cause = e.root_cause();
                println!("{}", style(format!("ERROR: {}: {}", e, root_cause)).red());
                return Err(e);
            }
        }
    };

    let environment_name = if let Some(environment_name) = &args.environment {
        environment_name.to_string()
    } else if let Ok(environment_name) = env::var("TDK_ENVIRONMENT") {
        environment_name
    } else {
        "default".to_string()
    };

    // Instantiate TDK
    let tdk = TDKSharedState::default().await;

    let mut environment =
        TDKEnvironments::fetch_from_file(args.path_environments.as_deref(), &environment_name)?;
    println!("Using Environment: {}", environment_name);

    let mut atm_config = ATMConfig::builder();
    atm_config = atm_config.with_ssl_certificates(&mut environment.ssl_certificates);

    let mut additional_secrets = Vec::new();
    let concierge_did = config.concierge.lock().await.agent.did.clone();
    let concierge_secret = get_secrets(&concierge_did)?.first().unwrap().to_owned();
    additional_secrets.push(concierge_secret);

    let mut model_names = Vec::new();
    {
        for (model_name, model) in config.models.lock().await.iter() {
            model_names.push(model_name.to_string());

            let did = model.lock().await.dids.first().unwrap().to_owned();
            let model_secret = get_secrets(&did.did)?.first().unwrap().to_owned();
            additional_secrets.push(model_secret);
        }
    }
    println!("additional_secrets: {}", additional_secrets.len());
    tdk.secrets_resolver.insert_vec(&additional_secrets).await;

    // Create a new ATM Client
    let atm = ATM::new(atm_config.build()?, tdk).await?;

    let mut model_profiles = HashMap::new();
    {
        for (model_name, model) in config.models.lock().await.iter() {
            let mut model_atm_profiles = Vec::new();
            for did in model.lock().await.dids.to_owned() {
                model_atm_profiles.push(
                    ATMProfile::new(
                        &atm,
                        Some(did.name),
                        did.did.to_string(),
                        Some(config.mediator_did.to_string()),
                    )
                    .await?,
                );
            }
            model_profiles.insert(model_name.to_owned(), model_atm_profiles);
        }
    }

    let (to_concierge, from_main) = mpsc::unbounded_channel::<ConciergeMessage>();
    let (terminator, mut interrupt_rx) = create_termination();
    let (concierge, _) = Concierge::new(atm.clone(), config.clone(), from_main);

    let concierge_profile = {
        ATMProfile::new(
            &atm,
            Some("Affinidi Concierge".into()),
            concierge_did.to_string(),
            Some(config.mediator_did.to_string()),
        )
        .await?
    };

    let concierge_handle = concierge.run(
        concierge_profile,
        model_profiles,
        terminator,
        interrupt_rx.resubscribe(),
    );

    for model_name in model_names {
        to_concierge.send(ConciergeMessage::StartModel { model_name })?;
    }

    try_join!(concierge_handle)?;

    match interrupt_rx.recv().await {
        Ok(reason) => match reason {
            Interrupted::UserInt => info!("exited per user request"),
            Interrupted::OsSigInt => info!("exited because of an os sig int"),
            Interrupted::SystemError => info!("exited because of a system error"),
        },
        _ => {
            println!("exited because of an unexpected error");
        }
    }

    Ok(())
}
