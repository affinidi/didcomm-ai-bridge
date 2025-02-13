/*!
 * Instantiates an AI Model DIDComm Agent endpoint
 *
 * Allows for interaction with a AI model via DIDComm messages
 */

use std::{collections::HashMap, sync::Arc};

use crate::{
    agents::state_management::ChatChannelState,
    chat_messages::handle_message,
    didcomm_messages::clear_messages::{clear_inbound_messages, clear_outbound_messages},
    termination::Interrupted,
};
use affinidi_messaging_didcomm::{Message, UnpackMetadata};
use affinidi_messaging_sdk::{ATM, profiles::Profile};
use anyhow::Result;
use sha256::digest;
use tokio::{
    select,
    sync::{
        Mutex,
        mpsc::{self, UnboundedReceiver, UnboundedSender},
    },
    task::JoinHandle,
};
use tracing::{info, warn};

use super::state_management::OllamaModel;

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
    model: Arc<Mutex<OllamaModel>>,
}

impl ModelAgent {
    /// Create a new Model Agent
    /// Returns Model Agent
    pub fn new(
        atm: ATM,
        model: Arc<Mutex<OllamaModel>>,
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

    pub async fn start(self, profiles: Vec<Profile>) -> Result<JoinHandle<()>> {
        let agent = ModelAgent {
            atm: self.atm.clone(),
            concierge_tx: self.concierge_tx.clone(),
            to_model_channel: self.to_model_channel,
            model: self.model.clone(),
        };

        let handle = tokio::spawn(async move {
            let _ = agent.run(profiles).await;
        });

        Ok(handle)
    }

    /// Run the Model Agent
    async fn run(mut self, profiles: Vec<Profile>) -> Result<Interrupted> {
        let model_name = { self.model.lock().await.name.clone() };
        let (direct_tx, mut direct_rx) = mpsc::channel::<Box<(Message, UnpackMetadata)>>(32);

        info!("Model ({}) starting...", model_name);
        let mut activated_profiles: HashMap<String, Arc<Profile>> = HashMap::new();
        for profile in profiles {
            let model_profile = self.atm.profile_add(&profile, false).await?;
            activated_profiles.insert(profile.inner.did.clone(), model_profile.clone());

            let _ = clear_inbound_messages(&self.atm, &model_profile).await;
            let _ = clear_outbound_messages(&self.atm, &model_profile).await;

            // Start live streaming
            self.atm.profile_enable_websocket(&model_profile).await?;
            model_profile
                .enable_direct_channel(direct_tx.clone())
                .await?;
            info!(
                "Model ({}) Profile Activated: {}",
                model_name, profile.inner.did
            );
        }

        info!("Model ({}) Started", model_name);

        let result = loop {
            select! {
                Some(action) = self.to_model_channel.recv() => match action {
                ModelAction::Exit => {
                    info!("Model Exiting...");

                    break Interrupted::UserInt;
                },
            },
                Some(boxed_data) = direct_rx.recv() => {
                        let (message, meta) = *boxed_data;

                        let Some(from_did) = message.from.clone() else {
                            warn!("Received anonymous message, can't reply. Ignoring...");
                            continue;
                        };
                        let from_did_hash = digest(&from_did);

                        let model_name = {
                            let mut model = self.model.lock().await;
                            if model.channel_state.get_mut(&from_did_hash).is_none() {
                                    let remote_state = ChatChannelState {
                                        remote_did_hash: from_did_hash.clone(),
                                        remote_did: from_did.clone(),
                                        ..Default::default()
                                    };
                                    model.channel_state.insert(from_did_hash.clone(), remote_state);
                            }
                            model.name.clone()
                        };

                        let to_did = message.to.as_ref().unwrap().first().unwrap().clone();
                        let profile = match activated_profiles.get(&to_did) {
                            Some(profile) => profile,
                            None => {
                                warn!("Received message from unknown DID: ({}). Ignoring...", to_did);
                                continue;
                            }
                        };

                       let _ = handle_message(&self.atm,  profile, &self.model, &model_name, &message).await;
                       let _ = self.atm.delete_message_background(profile, &meta.sha256_hash).await;
                },
            }
        };

        info!("{}: Exiting Model Agent", model_name);

        Ok(result)
    }
}
