use affinidi_tdk::secrets_resolver::secrets::{Secret, SecretMaterial, SecretType};
use anyhow::{Context, Result};
use base64::{Engine, prelude::BASE64_STANDARD_NO_PAD};
use console::style;
use did_peer::{
    DIDPeer, DIDPeerCreateKeys, DIDPeerKeys, DIDPeerService, PeerServiceEndPoint,
    PeerServiceEndPointLong,
};
use keyring::Entry;
use ssi::{JWK, jwk::Params};

pub mod activate;
pub mod agents;
pub mod chat_messages;
pub mod didcomm_messages;
pub mod termination;

const DIDCOMM_AI_BRIDGE_KEYRING_SERVICE_NAME: &str = "didcomm-ai-bridge";

pub enum DIDMethods {
    Key,
    Peer,
}

pub fn create_did(method: &DIDMethods, mediator_did: &str) -> Result<String> {
    match method {
        DIDMethods::Key => _create_did_key(),
        DIDMethods::Peer => _create_did_peer(mediator_did),
    }
}

// Fetches the secret from keyring
pub fn get_did_secret(did: &str) -> Result<Vec<u8>> {
    let entry = Entry::new(DIDCOMM_AI_BRIDGE_KEYRING_SERVICE_NAME, did)?;
    match entry.get_secret() {
        Ok(secret) => Ok(secret),
        Err(e) => {
            println!(
                "{}",
                style(format!("ERROR: Couldn't get secret for {}: {}", did, e)).red()
            );
            return Err(e.into());
        }
    }
}

// Deletes the secret from keyring
pub fn delete_did_secret(did: &str) -> Result<()> {
    let event = Entry::new(DIDCOMM_AI_BRIDGE_KEYRING_SERVICE_NAME, did).unwrap();
    let _ = event.delete_credential();
    Ok(())
}

/// Creates a DID Key to use as the DIDComm agent for a Ollama Model
fn _create_did_key() -> Result<String> {
    let secp256k1_key = JWK::generate_secp256k1();
    let did_key = ssi::dids::DIDKey::generate(&secp256k1_key)
        .unwrap()
        .to_string();

    let mut secrets = Vec::new();
    if let Params::EC(map) = secp256k1_key.params {
        secrets.push(Secret {
            id: [&did_key, "#", &did_key[8..]].concat(),
            type_: SecretType::JsonWebKey2020,
            secret_material: SecretMaterial::JWK {
                private_key_jwk: serde_json::json!({
                     "crv": map.curve,
                     "kty": "EC",
                     "x": String::from(map.x_coordinate.clone().unwrap()),
                     "y": String::from(map.y_coordinate.clone().unwrap()),
                     "d": String::from(map.ecc_private_key.clone().unwrap())
                }),
            },
        });
    }

    let entry = Entry::new(DIDCOMM_AI_BRIDGE_KEYRING_SERVICE_NAME, &did_key)?;
    entry.set_secret(
        BASE64_STANDARD_NO_PAD
            .encode(serde_json::to_string(&secrets).unwrap().as_bytes())
            .as_bytes(),
    )?;

    Ok(did_key)
}

/// Creates a DID Peer to use as the DIDComm agent for a LLM
fn _create_did_peer(mediator_did: &str) -> Result<String> {
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
    let (did_peer, _) =
        DIDPeer::create_peer_did(&keys, Some(&services)).context("Failed to create did:peer")?;

    // Save the private keys to secure storage

    let mut secrets = Vec::new();
    if let Params::OKP(map) = v_ed25519_key.params {
        secrets.push(Secret {
            id: [&did_peer, "#key-1"].concat(),
            type_: SecretType::JsonWebKey2020,
            secret_material: SecretMaterial::JWK {
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
            secret_material: SecretMaterial::JWK {
                private_key_jwk: serde_json::json!({
                     "crv": map.curve, "kty": "EC", "x": String::from(map.x_coordinate.clone().unwrap()), "y": String::from(map.y_coordinate.clone().unwrap()), "d": String::from(map.ecc_private_key.clone().unwrap())
                }),
            },
        });
    }

    let entry = Entry::new(DIDCOMM_AI_BRIDGE_KEYRING_SERVICE_NAME, &did_peer)?;
    entry.set_secret(
        BASE64_STANDARD_NO_PAD
            .encode(serde_json::to_string(&secrets).unwrap().as_bytes())
            .as_bytes(),
    )?;

    Ok(did_peer)
}
