/*
 * Handles the conversion of the configuration into DIDComm profiles and starting to listen.
 */

use affinidi_messaging_sdk::{
    messages::{fetch::FetchOptions, FetchDeletePolicy},
    profiles::Profile,
    secrets::Secret,
    ATM,
};
use anyhow::Result;
use base64::{prelude::BASE64_STANDARD_NO_PAD, Engine};
use console::style;
use keyring::Entry;

use crate::{config::OllamaModel, Config};

/// Starts DIDComm agents for each of the configured models
pub async fn activate_agents(atm: &ATM, config: &Config) -> Result<()> {
    for (model_name, model) in &config.models {
        let profile = _create_profile(atm, model_name, model, &config.mediator_did).await?;

        print!(
            "{}",
            style(format!("Activating DIDComm Profile {}", model_name)).green()
        );
        // Activate the profile, but live streaming is off
        let profile = atm.profile_add(&profile, false).await?;

        // Clear out the inbox queue in case old questions have been queued up
        atm.fetch_messages(
            &profile,
            &FetchOptions {
                delete_policy: FetchDeletePolicy::Optimistic,
                ..Default::default()
            },
        )
        .await?;

        // Start live streaming
        atm.profile_enable_websocket(&profile).await?;

        println!(" {}", style(":: activated!").green());
    }
    Ok(())
}

/// Create a DIDComm profile for the given model
async fn _create_profile(
    atm: &ATM,
    model_name: &str,
    model: &OllamaModel,
    mediator_did: &str,
) -> Result<Profile> {
    let secrets = _get_secrets(&model.did)?;
    let profile = Profile::new(
        atm,
        Some(model_name.to_string()),
        model.did.clone(),
        Some(mediator_did.to_string()),
        secrets,
    )
    .await?;
    Ok(profile)
}

/// Retrieves secrets for a DID from the keyring
fn _get_secrets(did: &str) -> Result<Vec<Secret>> {
    // Fetch the secret from keyring
    let entry = Entry::new("didcomm-ollama", did)?;
    let raw_secrets = match entry.get_secret() {
        Ok(secret) => secret,
        Err(e) => {
            println!(
                "{}",
                style(format!("ERROR: Couldn't get secret for {}: {}", did, e)).red()
            );
            return Err(e.into());
        }
    };

    // Decode the secret
    let raw_secrets: String = match BASE64_STANDARD_NO_PAD.decode(raw_secrets) {
        Ok(secret) => match String::from_utf8(secret) {
            Ok(secret_str) => secret_str,
            Err(e) => {
                println!(
                    "{}",
                    style(format!(
                        "ERROR: Couldn't convert secret to utf8 for {}: {}",
                        did, e
                    ))
                    .red()
                );
                return Err(e.into());
            }
        },
        Err(e) => {
            println!(
                "{}",
                style(format!("ERROR: Couldn't decode secret for {}: {}", did, e)).red()
            );
            return Err(e.into());
        }
    };

    match serde_json::from_str(&raw_secrets) {
        Ok(secrets) => Ok(secrets),
        Err(e) => {
            println!(
                "{}",
                style(format!("ERROR: Couldn't parse secrets for {}: {}", did, e)).red()
            );
            Err(e.into())
        }
    }
}
