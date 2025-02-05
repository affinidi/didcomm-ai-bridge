/*!
 * Handles the concierge service for the Ollama Bridge
 *
 * Concierge allows for self management of the Ollama Bridge
 */

use std::collections::HashMap;

use crate::{
    activate::create_model_profile,
    agents::model::{ModelAction, ModelAgent},
    chat_messages::send_message,
    config::OllamaModel,
    didcomm_messages::{
        clear_messages::clear_inbound_messages, handle_presence,
        oob_connection::send_connection_response,
    },
    termination::{Interrupted, Terminator},
};
use affinidi_messaging_didcomm::{Message, UnpackMetadata};
use affinidi_messaging_sdk::{profiles::Profile, ATM};
use anyhow::Result;
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
    StartModel { model: OllamaModel },
}

/// Concierge Task
pub struct Concierge {
    /// Affinidi Messaging SDK
    atm: ATM,
    /// Channel that concierge uses to receive messages from other tasks
    to_concierge_channel: UnboundedReceiver<ConciergeMessage>,
    /// Mediator DID
    mediator_did: String,
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
        mediator_did: String,
        to_concierge: UnboundedReceiver<ConciergeMessage>,
    ) -> (Self, UnboundedReceiver<ConciergeMessage>) {
        let (_, from_concierge) = mpsc::unbounded_channel::<ConciergeMessage>();
        (
            Self {
                atm,
                mediator_did,
                to_concierge_channel: to_concierge,
            },
            from_concierge,
        )
    }

    /// Run the Concierge Task
    pub async fn run(
        mut self,
        profile: Profile,
        mut terminator: Terminator,
        mut interrupt_rx: broadcast::Receiver<Interrupted>,
    ) -> Result<Interrupted> {
        let profile = self.atm.profile_add(&profile, false).await?;
        let _ = clear_inbound_messages(&self.atm, &profile).await;

        // Start live streaming
        self.atm.profile_enable_websocket(&profile).await?;

        info!("Concierge Task Started");
        let (direct_tx, mut direct_rx) = mpsc::channel::<Box<(Message, UnpackMetadata)>>(32);

        profile.enable_direct_channel(direct_tx).await?;

        let mut models: HashMap<String, Model> = HashMap::new();
        // Channels used to communicate from models to the concierge
        let (to_concierge_from_models, mut from_models_to_concierge) =
            mpsc::unbounded_channel::<ModelAction>();

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
                ConciergeMessage::StartModel { model } => {
                    info!("Starting Model: {:?}", model.name);
                    let model_profile = create_model_profile(&self.atm, &model.name, &model, &self.mediator_did).await?;
                    let model_profile = self.atm.profile_add(&model_profile, false).await?;
                    info!("Model Profile Created: {:?}", model_profile.inner.did);
                    // Channel to communicate with the model
                    let (to_model, from_concierge) = mpsc::unbounded_channel::<ModelAction>();

                    let model_agent = ModelAgent::new(self.atm.clone(), model.clone(), from_concierge, to_concierge_from_models.clone());
                    info!("Model Agent new: {}", &model.name);
                    model_agent.start(model_profile).await?;

                    info!("After run(): {}", &model.name);
                    models.insert(model.name.clone(), Model {  tx_channel: to_model});
                }
            },
                Some(boxed_data) = direct_rx.recv() => {
                        let (message, meta) = *boxed_data;
                        let _ = self.atm.delete_message_background(&profile, &meta.sha256_hash).await;

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
                        } else if message.type_ ==  "https://affinidi.com/atm/client-actions/chat-presence" {
                            // Send a presence response back
                            handle_presence(&self.atm, &profile, &message).await;
                        } else {
                            info!("Concierge Received Message: {:#?}", message);
                            let _ = send_message(
                                &self.atm,
                                &profile,
                                "I am an unintelligent response from a very intelligent concierge",
                                &message.from.unwrap(),
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

        //join_all(handles).await;

        Ok(result)
    }
}
