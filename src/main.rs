/*!
 * Creates a DIDComm agent for each Ollama model allowing for private and secure chat interface.
 *
 */

use activate::activate_agents;
use affinidi_messaging_sdk::{
    config::Config as ATMConfig, transports::websockets::ws_handler::WsHandlerMode, ATM,
};
use anyhow::Result;
use chat_messages::handle_message;
use clap::Parser;
use config::Config;
use console::style;
use setup_wizard::{add_new_model, run_setup_wizard};
use tracing_subscriber::filter;

mod activate;
mod chat_messages;
mod config;
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

    let mut config = match Config::load(&config_file) {
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

    // Add a new model to the configuration
    if args.add_model {
        add_new_model(&mut config).await?;
        config.save(&config_file)?;
    }

    if config.models.is_empty() {
        println!("{}", style("No models configured. You may want to run the setup wizard, or add a model manually.").yellow());
        return Ok(());
    }

    let atm = ATM::new(
        ATMConfig::builder()
            .with_ws_handler_mode(WsHandlerMode::DirectChannel)
            .build()
            .unwrap(),
    )
    .await?;

    // Configuration is all set, Activate each model DIDComm agent
    activate_agents(&atm, &config).await?;

    let mut incoming = if let Some(channel) = atm.get_inbound_channel() {
        channel
    } else {
        return Err(anyhow::anyhow!("No inbound channel found"));
    };

    loop {
        match incoming.recv().await {
            Ok((msg, _)) => {
                handle_message(&config, &atm, &msg).await;
            }
            Err(e) => {
                println!("ERROR: {:?}", e);
                return Err(e.into());
            }
        }
    }
}
