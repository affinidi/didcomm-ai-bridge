/*
 * Handles the conversion of the configuration into DIDComm profiles and starting to listen.
 */

use std::sync::Arc;

use affinidi_messaging_sdk::{profiles::Profile, secrets::Secret, ATM};
use anyhow::Result;
use base64::{prelude::BASE64_STANDARD_NO_PAD, Engine};
use console::style;
use keyring::Entry;
use tokio::sync::Mutex;

use crate::agents::state_management::OllamaModel;

/// Create a DIDComm profile for the given model
pub async fn create_model_profile(
    atm: &ATM,
    model_name: &str,
    model: &Arc<Mutex<OllamaModel>>,
    mediator_did: &str,
) -> Result<Profile> {
    let model = model.lock().await;
    let secrets = get_secrets(&model.did)?;
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
pub fn get_secrets(did: &str) -> Result<Vec<Secret>> {
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
