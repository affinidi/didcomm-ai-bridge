use affinidi_messaging_didcomm::Message;
use affinidi_messaging_sdk::{profiles::Profile, protocols::Protocols, ATM};
use chrono::Local;
use serde_json::json;
use std::{sync::Arc, time::SystemTime};
use tracing::warn;

pub mod clear_messages;
pub mod oob_connection;

pub async fn handle_presence(atm: &ATM, profile: &Arc<Profile>, to_did: &str) {
    // Create the response message
    // presence timestamp = 2025-02-05T04:59:09.190394Z
    //                      2025-02-05T14:33:37.816332+08:00
    let dt = Local::now();
    let new_message = Message::build(
        uuid::Uuid::new_v4().to_string(),
        "https://affinidi.com/atm/client-actions/chat-presence".to_string(),
        json!({"presence": dt.to_rfc3339()}),
    )
    .from(profile.inner.did.clone())
    .to(to_did.to_string())
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
            to_did,
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
            to_did,
            Some(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 30,
            ),
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
        Ok(_) => {}
        Err(e) => warn!("Error Sending Presence: {:#?}", e),
    }
}
