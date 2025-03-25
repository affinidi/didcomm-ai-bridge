/*
 * Handles the conversion of the configuration into DIDComm profiles and starting to listen.
 */

use affinidi_tdk::secrets_resolver::secrets::Secret;
use anyhow::Result;
use base64::{Engine, prelude::BASE64_STANDARD_NO_PAD};
use console::style;

use crate::get_did_secret;

/// Retrieves secrets for a DID from the keyring
pub fn get_secrets(did: &str) -> Result<Vec<Secret>> {
    let raw_secrets = match get_did_secret(did) {
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
