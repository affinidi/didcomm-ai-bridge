/*!
 * DIDComm Out Of Band (OOB) Discovery and Connection handling
 */

use std::{sync::Arc, time::SystemTime};

use affinidi_messaging_didcomm::{Attachment, Message};
use affinidi_messaging_sdk::{profiles::Profile, protocols::Protocols, ATM};
use anyhow::Result;
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Name {
    pub given: Option<String>,
    pub surname: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VcardType {
    pub r#type: VcardTypes,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum VcardTypes {
    #[serde(rename = "work")]
    Work(String),
    #[serde(rename = "cell")]
    Cell(String),
}
#[derive(Debug, Serialize, Deserialize)]
pub struct VCard {
    pub n: Name,
    pub email: Option<VcardType>,
    pub tel: Option<VcardType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub photo: Option<String>,
}

// Reads a file and returns a BAS64 encoded String
fn _read_file(path: &str) -> String {
    let file = std::fs::read(path).unwrap();
    BASE64_URL_SAFE_NO_PAD.encode(file)
}

pub async fn send_connection_response(
    atm: &ATM,
    profile: &Arc<Profile>,
    message: &Message,
) -> Result<String> {
    // Get the new DID
    let new_did = message
        .body
        .get("channel_did")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    let photo = if profile.inner.alias.starts_with("deepseek") {
        _read_file("deepseek.png")
    } else {
        _read_file("ollama.png")
    };

    let vcard = VCard {
        n: Name {
            given: Some(profile.inner.alias.clone()),
            surname: Some(String::new()),
        },
        email: Some(VcardType {
            r#type: VcardTypes::Work(String::new()),
        }),
        tel: Some(VcardType {
            r#type: VcardTypes::Cell(String::new()),
        }),
        photo: Some(photo),
    };
    let vcard = serde_json::to_string(&vcard).unwrap();
    let attachment = Attachment::base64(BASE64_URL_SAFE_NO_PAD.encode(vcard))
        .id(Uuid::new_v4().into())
        .description("Affinidi Concierge vCard Info".into())
        .media_type("text/x-vcard".into())
        .format("https://affinidi.com/atm/client-attachment/contact-card".into())
        .finalize();

    // Create the response message
    let new_message = Message::build(
        uuid::Uuid::new_v4().to_string(),
        "https://affinidi.com/atm/client-actions/connection-accepted".to_string(),
        json!({"channel_did": profile.inner.did.clone()}),
    )
    .from(profile.inner.did.clone())
    .pthid(message.pthid.clone().unwrap())
    .thid(message.thid.clone().unwrap())
    .to(message.from.clone().unwrap())
    .attachment(attachment)
    .created_time(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .finalize();

    let packed = atm
        .pack_encrypted(
            &new_message,
            message.from.as_ref().unwrap(),
            Some(&profile.inner.did),
            Some(&profile.inner.did),
        )
        .await;

    let protocols = Protocols::default();
    let forwarded = protocols
        .routing
        .forward_message(
            atm,
            profile,
            packed.unwrap().0.as_str(),
            profile.dids().unwrap().1,
            message.from.as_ref().unwrap(),
            None,
            None,
        )
        .await;

    match atm
        .send_message(
            profile,
            &forwarded.as_ref().unwrap().1,
            &forwarded.as_ref().unwrap().0,
            false,
            true,
        )
        .await
    {
        Ok(_) => info!("Connection Response Sent"),
        Err(e) => warn!("Error Sending Connection Response: {:#?}", e),
    }

    Ok(new_did)
}
