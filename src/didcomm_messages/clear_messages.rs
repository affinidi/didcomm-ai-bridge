use std::sync::Arc;

use affinidi_messaging_sdk::{
    messages::{fetch::FetchOptions, DeleteMessageRequest, FetchDeletePolicy, Folder},
    profiles::Profile,
    ATM,
};
use anyhow::Result;
use tracing::info;

pub async fn clear_inbound_messages(atm: &ATM, profile: &Arc<Profile>) -> Result<()> {
    // Clear out the inbox queue in case old questions have been queued up
    let mut deleted = 0;
    loop {
        let response = atm
            .fetch_messages(
                profile,
                &FetchOptions {
                    delete_policy: FetchDeletePolicy::Optimistic,
                    ..Default::default()
                },
            )
            .await?;

        if response.success.is_empty() {
            break;
        } else {
            deleted += response.success.len();
        }
    }

    info!(
        "{}: Cleared ({}) messages from INBOX",
        profile.inner.alias, deleted
    );

    Ok(())
}

pub async fn clear_outbound_messages(atm: &ATM, profile: &Arc<Profile>) -> Result<()> {
    // Clear out the outbox queue in case old questions have been queued up
    let mut deleted = 0;
    loop {
        let response = atm.list_messages(profile, Folder::Outbox).await?;

        let mut request = DeleteMessageRequest::default();
        if response.is_empty() {
            break;
        } else {
            for message in &response {
                request.message_ids.push(message.msg_id.clone());
            }
            deleted += response.len();
            let _ = atm.delete_messages_direct(profile, &request).await?;
        }
    }

    info!(
        "{}: Cleared ({}) messages from OUTBOX",
        profile.inner.alias, deleted
    );

    Ok(())
}
