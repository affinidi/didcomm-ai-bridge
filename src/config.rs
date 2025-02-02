/*!
 * Configuration information relating to DIDComm-Ollama bridge
 */

use affinidi_messaging_sdk::secrets::{Secret, SecretType};
use anyhow::{Context, Result};
use base64::{prelude::BASE64_STANDARD_NO_PAD, Engine};
use console::style;
use did_peer::{
    DIDPeer, DIDPeerCreateKeys, DIDPeerKeys, DIDPeerService, PeerServiceEndPoint,
    PeerServiceEndPointLong,
};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use ssi::{jwk::Params, JWK};
use std::{collections::HashMap, fs};

/// Configuration for the DIDComm-Ollama bridge
#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Config {
    /// Ollama models that have been configured
    pub(crate) models: HashMap<String, OllamaModel>,
    /// Mediator DID for DIDComm
    pub(crate) mediator_did: String,
}

impl Config {
    /// Load the configuration from the specified file
    pub fn load(config_file: &str) -> Result<Config> {
        let contents = fs::read_to_string(config_file).context(format!(
            "Couldn't open configuration file ({})",
            config_file
        ))?;

        serde_json::from_str(&contents)
            .map_err(anyhow::Error::msg)
            .context(format!(
                "Parse error on configuration file ({})",
                config_file
            ))
    }

    /// Save the configuration to the specified file
    pub fn save(&self, config_file: &str) -> Result<()> {
        let contents =
            serde_json::to_string_pretty(&self).context("Couldn't serialize configuration")?;
        fs::write(config_file, contents).context(format!(
            "Couldn't write configuration file ({})",
            config_file
        ))
    }

    /// Add a Ollama model to the configuration
    pub fn add_model(&mut self, name: &str, model: OllamaModel) {
        self.models.insert(name.into(), model);
    }

    /// Remove a Ollama model from the configuration
    pub fn remove_model(&mut self, model_name: &str) {
        if let Some(model) = self.models.remove(model_name) {
            // Clean up secret keys
            let event = Entry::new("didcomm-ollama", &model.did).unwrap();
            let _ = event.delete_credential();
        }
    }
}

// ************************************************************************************************
//  OllamaModel
// ************************************************************************************************

/// OllamaModel represents a model within the Ollama Service
#[derive(Deserialize, Serialize)]
pub(crate) struct OllamaModel {
    /// Name of the model in Ollama
    pub name: String,
    /// Hostname of the Ollama service for this model
    pub ollama_host: String,
    /// Port of the Ollama service for this model
    pub ollama_port: u16,
    /// DID for sending messages to this model
    pub did: String,
}

impl OllamaModel {
    pub fn new(
        ollama_host: String,
        ollama_port: u16,
        mediator_did: &str,
        model_name: &str,
    ) -> Result<Self> {
        Ok(Self {
            name: model_name.into(),
            ollama_host,
            ollama_port,
            did: OllamaModel::_create_did_peer(mediator_did, model_name)?,
        })
    }

    /// Creates a DID Peer to use as the DIDComm agent for a Ollama Model
    fn _create_did_peer(mediator_did: &str, model_name: &str) -> Result<String> {
        let e_secp256k1_key = JWK::generate_secp256k1();
        let v_ed25519_key = JWK::generate_ed25519().unwrap();

        let e_did_key = ssi::dids::DIDKey::generate(&e_secp256k1_key).unwrap();
        let v_did_key = ssi::dids::DIDKey::generate(&v_ed25519_key).unwrap();

        let keys = vec![
            DIDPeerCreateKeys {
                purpose: DIDPeerKeys::Verification,
                type_: None,
                public_key_multibase: Some(v_did_key[8..].to_string()),
            },
            DIDPeerCreateKeys {
                purpose: DIDPeerKeys::Encryption,
                type_: None,
                public_key_multibase: Some(e_did_key[8..].to_string()),
            },
        ];

        // Create a service definition
        let services = vec![DIDPeerService {
            _type: "dm".into(),
            service_end_point: PeerServiceEndPoint::Long(PeerServiceEndPointLong {
                uri: mediator_did.into(),
                accept: vec!["didcomm/v2".into()],
                routing_keys: vec![],
            }),
            id: None,
        }];

        // Create the did:peer DID
        let (did_peer, _) = DIDPeer::create_peer_did(&keys, Some(&services))
            .context("Failed to create did:peer")?;

        println!(
            "{} {}",
            style("Created a new DID Peer for model ").blue(),
            style(model_name).green()
        );
        // Save the private keys to secure storage

        let mut secrets = Vec::new();
        if let Params::OKP(map) = v_ed25519_key.params {
            secrets.push(Secret {
                id: [&did_peer, "#key-1"].concat(),
                type_: SecretType::JsonWebKey2020,
                secret_material: affinidi_messaging_sdk::secrets::SecretMaterial::JWK {
                    private_key_jwk: serde_json::json!({
                         "crv": map.curve, "kty": "OKP", "x": String::from(map.public_key.clone()), "d": String::from(map.private_key.clone().unwrap())}
                    ),
                },
            });
        }

        if let Params::EC(map) = e_secp256k1_key.params {
            secrets.push(Secret {
                id: [&did_peer, "#key-2"].concat(),
                type_: SecretType::JsonWebKey2020,
                secret_material: affinidi_messaging_sdk::secrets::SecretMaterial::JWK {
                    private_key_jwk: serde_json::json!({
                         "crv": map.curve, "kty": "EC", "x": String::from(map.x_coordinate.clone().unwrap()), "y": String::from(map.y_coordinate.clone().unwrap()), "d": String::from(map.ecc_private_key.clone().unwrap())
                    }),
                },
            });
        }

        let entry = Entry::new("didcomm-ollama", &did_peer)?;
        entry.set_secret(
            BASE64_STANDARD_NO_PAD
                .encode(serde_json::to_string(&secrets).unwrap().as_bytes())
                .as_bytes(),
        )?;

        Ok(did_peer)
    }
}
