/*!
 * Creates a DIDComm agent for each Ollama model allowing for private and secure chat interface.
 *
 */

use affinidi_messaging_sdk::{config::Config as ATMConfig, profiles::Profile, ATM};
use anyhow::Result;
use clap::Parser;
use console::style;
use didcomm_ollama::{
    activate::get_secrets,
    agents::concierge::{Concierge, ConciergeMessage},
    config::Config,
    termination::{create_termination, Interrupted},
};
use setup_wizard::run_setup_wizard;
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

    let config = match Config::load(&config_file) {
        Ok(config) => config,
        Err(e) => {
            if e.to_string()
                .starts_with("Couldn't open configuration file")
            {
                println!("{}", style("ERROR: No configuration file found.").red());
                let config = run_setup_wizard().await?;
                config.save(&config_file)?;
                config
            } else {
                let root_cause = e.root_cause();
                println!("{}", style(format!("ERROR: {}: {}", e, root_cause)).red());
                return Err(e);
            }
        }
    };

    let atm = ATM::new(
        ATMConfig::builder()
            //.with_ws_handler_mode(WsHandlerMode::DirectChannel)
            .build()
            .unwrap(),
    )
    .await?;

    let (to_concierge, from_main) = mpsc::unbounded_channel::<ConciergeMessage>();
    let (terminator, mut interrupt_rx) = create_termination();
    let (concierge, _) = Concierge::new(atm.clone(), config.mediator_did.clone(), from_main);

    let concierge_profile = Profile::new(
        &atm,
        Some("Affinidi Concierge".into()),
        config.concierge_did.to_string(),
        Some(config.mediator_did.to_string()),
        get_secrets(&config.concierge_did)?,
    )
    .await?;
    let concierge_handle = concierge.run(concierge_profile, terminator, interrupt_rx.resubscribe());

    for (_, model) in config.models {
        to_concierge.send(ConciergeMessage::StartModel { model })?;
    }

    try_join!(concierge_handle)?;

    if let Ok(reason) = interrupt_rx.recv().await {
        match reason {
            Interrupted::UserInt => info!("exited per user request"),
            Interrupted::OsSigInt => info!("exited because of an os sig int"),
            Interrupted::SystemError => info!("exited because of a system error"),
        }
    } else {
        println!("exited because of an unexpected error");
    }

    Ok(())
}
