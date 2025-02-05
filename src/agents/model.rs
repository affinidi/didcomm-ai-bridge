/*!
 * Instantiates an AI Model DIDComm Agent endpoint
 *
 * Allows for interaction with a AI model via DIDComm messages
 */

use std::sync::Arc;

use crate::{
    chat_messages::{handle_message, send_message},
    config::OllamaModel,
    didcomm_messages::oob_connection::send_connection_response,
    termination::Interrupted,
};
use affinidi_messaging_didcomm::{Message, UnpackMetadata};
use affinidi_messaging_sdk::{
    messages::{fetch::FetchOptions, FetchDeletePolicy},
    profiles::Profile,
    ATM,
};
use anyhow::Result;
use tokio::{
    select,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tracing::info;

/// Model Actions that can be sent to/from Model Task
#[derive(Debug)]
pub enum ModelAction {
    Exit,
}

/// Model Agent
pub struct ModelAgent {
    /// Affinidi Messaging SDK
    atm: ATM,
    /// Channel that this model uses to send messages to the concierge
    concierge_tx: UnboundedSender<ModelAction>,
    /// Channel that this model uses to receive messages from the concierge
    to_model_channel: UnboundedReceiver<ModelAction>,
    /// Model info
    model: OllamaModel,
}

impl ModelAgent {
    /// Create a new Model Agent
    /// Returns Model Agent
    pub fn new(
        atm: ATM,
        model: OllamaModel,
        to_model_channel: UnboundedReceiver<ModelAction>,
        to_concierge_channel: UnboundedSender<ModelAction>,
    ) -> Self {
        Self {
            atm,
            concierge_tx: to_concierge_channel,
            to_model_channel,
            model,
        }
    }

    pub async fn start(self, profile: Arc<Profile>) -> Result<JoinHandle<()>> {
        let agent = ModelAgent {
            atm: self.atm.clone(),
            concierge_tx: self.concierge_tx.clone(),
            to_model_channel: self.to_model_channel,
            model: self.model.clone(),
        };

        let handle = tokio::spawn(async move {
            let _ = agent.run(profile).await;
        });

        Ok(handle)
    }

    /// Run the Model Agent
    async fn run(mut self, profile: Arc<Profile>) -> Result<Interrupted> {
        info!("Model ({}) starting...", self.model.name);
        // Clear out the inbox queue in case old questions have been queued up
        self.atm
            .fetch_messages(
                &profile,
                &FetchOptions {
                    delete_policy: FetchDeletePolicy::Optimistic,
                    ..Default::default()
                },
            )
            .await?;

        info!("Model ({}) emptied queues", self.model.name);

        // Start live streaming
        self.atm.profile_enable_websocket(&profile).await?;

        info!("Model ({}) Started", self.model.name);
        let (direct_tx, mut direct_rx) = mpsc::channel::<Box<(Message, UnpackMetadata)>>(32);

        profile.enable_direct_channel(direct_tx).await?;

        let result = loop {
            select! {
                Some(action) = self.to_model_channel.recv() => match action {
                ModelAction::Exit => {
                    info!("Model Exiting...");

                    break Interrupted::UserInt;
                },
            },
                Some(boxed_data) = direct_rx.recv() => {
                        let (message, _) = *boxed_data;

                        if message.type_ == "https://affinidi.com/atm/client-actions/connection-setup" {
                            info!("Concierge Received Connection Setup Message: {:#?}", message);
                            let new_did = send_connection_response(&self.atm, &profile, &message).await?;
                            let _ = send_message(
                                &self.atm,
                                &profile,
                                "First Message from a very intelligent concierge",
                                &new_did,
                            )
                            .await;
                        } else {
                        info!("Model received message: {:?}", message);
                       handle_message(&self.atm, &profile, &self.model, &message).await;
                        }
                },
            }
        };

        Ok(result)
    }
}
