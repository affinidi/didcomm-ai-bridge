/*!
 * Handles the concierge service for the Ollama Bridge
 *
 * Concierge allows for self management of the Ollama Bridge
 */

use crate::{
    activate::create_model_profiles,
    agents::{
        model::{ModelAction, ModelAgent},
        state_management::{ChannelState, ChatChannelState, SharedState, SharedStateRef},
    },
    chat_messages::send_message,
    didcomm_messages::{
        clear_messages::{clear_inbound_messages, clear_outbound_messages},
        handle_presence,
        oob_connection::send_connection_response,
    },
    termination::{Interrupted, Terminator},
};
use affinidi_messaging_didcomm::{Message, UnpackMetadata};
use affinidi_messaging_sdk::{ATM, profiles::ATMProfile};
use anyhow::Result;
use console::style;
use sha256::digest;
use std::{collections::HashMap, sync::Arc};
use tokio::{
    select,
    sync::{
        broadcast,
        mpsc::{self, UnboundedReceiver, UnboundedSender},
    },
};
use tracing::{info, warn};

/// Concierge Messages that can be sent to/from Concierge Task
pub enum ConciergeMessage {
    Exit,
    StartModel { model_name: String },
}

/// Concierge Task
pub struct Concierge {
    /// Affinidi Messaging SDK
    atm: ATM,
    /// Channel that concierge uses to receive messages from other tasks
    to_concierge_channel: UnboundedReceiver<ConciergeMessage>,
    /// Mediator DID
    mediator_did: String,
    /// Shared State
    shared_state: SharedStateRef,
}

struct Model {
    // profile: Arc<Profile>,
    tx_channel: UnboundedSender<ModelAction>,
}

impl Concierge {
    /// Create a new Concierge Task
    /// Returns a tuple with the Concierge Task and a Receiver for messages from the Concierge Task
    pub fn new(
        atm: ATM,
        config: Arc<SharedState>,
        to_concierge: UnboundedReceiver<ConciergeMessage>,
    ) -> (Self, UnboundedReceiver<ConciergeMessage>) {
        let (_, from_concierge) = mpsc::unbounded_channel::<ConciergeMessage>();
        (
            Self {
                atm,
                mediator_did: config.mediator_did.clone(),
                to_concierge_channel: to_concierge,
                shared_state: config,
            },
            from_concierge,
        )
    }

    /// Run the Concierge Task
    pub async fn run(
        mut self,
        profile: ATMProfile,
        mut terminator: Terminator,
        mut interrupt_rx: broadcast::Receiver<Interrupted>,
    ) -> Result<Interrupted> {
        let profile = self.atm.profile_add(&profile, false).await?;
        let _ = clear_inbound_messages(&self.atm, &profile).await;
        let _ = clear_outbound_messages(&self.atm, &profile).await;

        // Start live streaming
        self.atm.profile_enable_websocket(&profile).await?;

        info!("Concierge Task Started");
        let (direct_tx, mut direct_rx) = mpsc::channel::<Box<(Message, UnpackMetadata)>>(32);

        profile.enable_direct_channel(direct_tx).await?;

        let mut models: HashMap<String, Model> = HashMap::new();
        // Channels used to communicate from models to the concierge
        let (to_concierge_from_models, mut from_models_to_concierge) =
            mpsc::unbounded_channel::<ModelAction>();

        let didcomm_agent = {
            let lock = self.shared_state.concierge.lock().await;
            lock.agent.clone()
        };

        let concierge_state = self.shared_state.concierge.clone();
        let result = loop {
            select! {
                Some(action) = from_models_to_concierge.recv() => {
                        warn!("NOT IMPLEMENTED: {:#?}", action);
                },
                Some(action) = self.to_concierge_channel.recv() => match action {
                ConciergeMessage::Exit => {
                    info!("Concierge Exiting...");
                    let _ = terminator.terminate(Interrupted::UserInt);

                    break Interrupted::UserInt;
                },
                ConciergeMessage::StartModel { model_name } => {
                    let model = {
                        let lock = self.shared_state.models.lock().await;
                        let Some(model) = lock.get(&model_name) else {
                            warn!("Model not found: {}", model_name);
                            continue;
                        };
                        model.clone()
                    };
                    info!("Starting Model: {:?}", model_name);
                    let model_profiles = create_model_profiles(&self.atm, &model_name, &model, &self.mediator_did).await?;
                    //let model_profile = self.atm.profile_add(&model_profile, false).await?;
                    // Channel to communicate with the model
                    let (to_model, from_concierge) = mpsc::unbounded_channel::<ModelAction>();

                    let model_agent = ModelAgent::new(self.atm.clone(), model.clone(), from_concierge, to_concierge_from_models.clone());
                    info!("Model Agent new: {}", &model_name);
                    model_agent.start(model_profiles).await?;

                    info!("After run(): {}", &model_name);
                    models.insert(model_name.clone(), Model {  tx_channel: to_model});
                }
            },
                Some(boxed_data) = direct_rx.recv() => {
                        let (message, meta) = *boxed_data;
                        let _ = self.atm.delete_message_background(&profile, &meta.sha256_hash).await;

                        let Some(from_did) = message.from.clone() else {
                            warn!("Received anonymous message, can't reply. Ignoring...");
                            continue;
                        };
                        let from_did_hash = digest(&from_did);

                        {
                            let mut concierge_state = concierge_state.lock().await;
                            if  concierge_state.get_channel_state(&from_did_hash).is_none() {
                                let remote_state = ChatChannelState {
                                    remote_did_hash: from_did_hash.clone(),
                                    remote_did: from_did.clone(),
                                    ..Default::default()
                                };
                                concierge_state.insert_channel_state(&from_did_hash, remote_state);
                            }
                        }

                        if message.type_ == "https://affinidi.com/atm/client-actions/connection-setup" {
                            info!(
                                "{}: Received Connection Setup Request: from({:#?})",
                                profile.inner.alias, message.from
                            );
                            let new_did = send_connection_response(&self.atm, &profile, &message, &didcomm_agent).await?;
                            {
                                let mut lock = concierge_state.lock().await;
                                let Some(from_did) = &message.from else {
                                    println!("{}", style("No 'from' field in message").red());
                                    println!(
                                        "{}",
                                        style("How would one respond to an anonymous message?").red()
                                    );
                                    return Err(anyhow::anyhow!("No 'from' field in message"));
                                };
                                let from_did_hash = digest(from_did);
                                lock.remove_channel_state(&from_did_hash);
                                let new_did_hash = digest(&new_did);
                                lock.insert_channel_state(
                                    &new_did_hash,
                                    ChatChannelState {
                                        remote_did: new_did.clone(),
                                        remote_did_hash: new_did_hash.clone(),
                                        ..Default::default()
                                    },
                                );
                            }
                            let _ = send_message(
                                &self.atm,
                                &profile,
                               &didcomm_agent.greeting,
                                &new_did,
                                &concierge_state,
                            )
                            .await;
                        } else if message.type_ ==  "https://affinidi.com/atm/client-actions/chat-presence" {
                            // Send a presence response back
                            let _ = handle_presence(&self.atm, &profile, &from_did).await;
                        } else if message.type_ ==  "https://affinidi.com/atm/client-actions/chat-delivered" {
                            // Ignore chat delivered messages
                        } else if message.type_ ==  "https://affinidi.com/atm/client-actions/chat-activity" {
                            // Ignore chat activity messages
                        } else if message.type_ ==  "https://didcomm.org/messagepickup/3.0/status" {
                            // Ignore DIDComm status messages
                        } else {
                            info!("Concierge Received Message: {:#?}", message);
                            let _ = send_message(
                                &self.atm,
                                &profile,
                                "I am an unintelligent response from a very intelligent concierge",
                                message.from.as_ref().unwrap(),
                                &concierge_state,
                            )
                            .await;
                        }
                },
                Ok(interrupted) = interrupt_rx.recv() => {
                    info!("Concierge Task Interrupted: {:?}", interrupted);
                    break interrupted;
                }
            }
        };

        // Clean up the models
        for (model_name, model) in models {
            let _ = model.tx_channel.send(ModelAction::Exit);
            info!("Send exit action to model: {}", model_name);
        }

        // Save the config to disk
        self.shared_state.save("config.json").await?;

        Ok(result)
    }
}
