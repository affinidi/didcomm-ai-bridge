use std::sync::Arc;

use affinidi_messaging_sdk::{
    messages::{fetch::FetchOptions, FetchDeletePolicy},
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
        "{}: Cleared ({}) messages from queue",
        profile.inner.alias, deleted
    );

    Ok(())
}
