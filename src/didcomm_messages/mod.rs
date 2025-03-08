use affinidi_messaging_didcomm::Message;
use affinidi_messaging_sdk::{ATM, profiles::ATMProfile};
use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::{sync::Arc, time::SystemTime};

pub mod clear_messages;
pub mod oob_connection;

pub async fn handle_presence(atm: &ATM, profile: &Arc<ATMProfile>, to_did: &str) -> Result<()> {
    // Create the response message
    // presence timestamp = 2025-02-05T04:59:09.190394Z
    //                      2025-02-05T14:33:37.816332+08:00
    let dt = Local::now();
    let id = uuid::Uuid::new_v4().to_string();
    let new_message = Message::build(
        id.clone(),
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
        .await?;

    if packed.1.messaging_service.is_none() {
        let _ = atm
            .forward_and_send_message(
                profile,
                &packed.0,
                None,
                profile.dids()?.1,
                to_did,
                None,
                None,
                false,
            )
            .await?;
    } else {
        let _ = atm
            .send_message(profile, &packed.0, &id, false, false)
            .await?;
    }
    Ok(())
}
