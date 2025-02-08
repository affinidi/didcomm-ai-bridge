/*!
 * All things to do with state management
 */

use crate::{create_did, DIDMethods};
use anyhow::{Context, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, sync::Arc};
use tokio::sync::Mutex as TokioMutex;

#[derive(Default)]
pub struct SharedState {
    /// Ollama models that have been configured
    pub models: Arc<TokioMutex<HashMap<String, Arc<TokioMutex<OllamaModel>>>>>,
    /// Mediator DID for DIDComm
    pub mediator_did: String,
    pub concierge: Arc<TokioMutex<ConciergeState>>,
}

pub type SharedStateRef = Arc<SharedState>;

/// Holding struct that eases conversion between JSON file and turning into shared state
#[derive(Default, Deserialize, Serialize)]
pub struct Config {
    pub models: HashMap<String, OllamaModel>,
    pub mediator_did: String,
    pub concierge: ConciergeState,
}

impl Config {
    pub fn from_config(self) -> SharedState {
        let models = self
            .models
            .into_iter()
            .map(|(name, model)| (name, Arc::new(TokioMutex::new(model))))
            .collect();

        SharedState {
            models: Arc::new(TokioMutex::new(models)),
            mediator_did: self.mediator_did,
            concierge: Arc::new(TokioMutex::new(self.concierge)),
        }
    }
}

// Common state for all Chat Channels
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct ChatChannelState {
    /// DID of the remote party
    pub remote_did: String,
    /// SHA256 hash of the remote DID
    pub remote_did_hash: String,
    /// activitySeqNo - used to show when the agent is thinking/typing
    pub activity_seq_no: u64,
    /// seqNo - used to track the order of messages when sent
    pub seq_no: u64,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct ConciergeState {
    /// Concierge Agent DID for DIDComm
    pub agent: DIDCommAgent,

    /// Remote Channels State
    pub channel_state: HashMap<String, ChatChannelState>,
}

/// DIDCommAgent represents an agent that can communicate using DIDComm
/// A model may have many listening agents
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct DIDCommAgent {
    pub did: String,
    pub name: String,
    pub greeting: String,
    pub image: String,
}

/// OllamaModel represents a model within the Ollama Service
#[derive(Clone, Serialize, Deserialize)]
pub struct OllamaModel {
    /// Name of the model in Ollama
    pub name: String,
    /// Hostname of the Ollama service for this model
    pub ollama_host: String,
    /// Port of the Ollama service for this model
    pub ollama_port: u16,
    /// DIDs for sending messages to this model
    pub dids: Vec<DIDCommAgent>,
    /// ChannelState for this model
    pub channel_state: HashMap<String, ChatChannelState>,
}

impl OllamaModel {
    pub fn new(
        ollama_host: String,
        ollama_port: u16,
        mediator_did: &str,
        model_name: &str,
        did_method: &DIDMethods,
    ) -> Result<Self> {
        Ok(Self {
            name: model_name.into(),
            ollama_host,
            ollama_port,
            dids: vec![DIDCommAgent {
                did: create_did(did_method, mediator_did)?,
                greeting: "Standard Greeting".into(),
                image: "deepseek.png".into(),
                name: model_name.into(),
            }],
            channel_state: HashMap::new(),
        })
    }
}

impl SharedState {
    pub fn load(config_file: &str) -> Result<Self> {
        let contents = fs::read_to_string(config_file).context(format!(
            "Couldn't open configuration file ({})",
            config_file
        ))?;

        let config: Config = serde_json::from_str(&contents)
            .map_err(anyhow::Error::msg)
            .context(format!(
                "Parse error on configuration file ({})",
                config_file
            ))?;

        Ok(config.from_config())
    }

    async fn to_config(&self) -> Result<Config> {
        let models = { self.models.lock().await.clone() };

        let mut new_models: HashMap<String, OllamaModel> = HashMap::new();
        for (name, model) in models {
            new_models.insert(name, model.lock().await.clone());
        }

        Ok(Config {
            models: new_models,
            mediator_did: self.mediator_did.clone(),
            concierge: self.concierge.lock().await.clone(),
        })
    }

    /// Save the configuration to the specified file
    pub async fn save(&self, config_file: &str) -> Result<()> {
        let contents = serde_json::to_string_pretty(&self.to_config().await?)
            .context("Couldn't serialize configuration")?;
        fs::write(config_file, contents).context(format!(
            "Couldn't write configuration file ({})",
            config_file
        ))
    }

    /// Add a Ollama model to the shared state
    pub async fn add_model(&mut self, name: &str, model: OllamaModel) {
        self.models
            .lock()
            .await
            .insert(name.into(), Arc::new(TokioMutex::new(model)));
    }

    /// Remove a Ollama model from the shared state
    pub async fn remove_model(&mut self, model_name: &str) {
        if let Some(model) = self.models.lock().await.remove(model_name) {
            // Clean up secret keys
            let lock = model.lock().await;
            for did in &lock.dids {
                let event = Entry::new("didcomm-ollama", &did.did).unwrap();
                let _ = event.delete_credential();
            }
        }
    }
}

/// Common way of getting ChatChannelState from OllamaModel or ConciergeState
pub trait ChannelState {
    /// Get a reference to the ChatChannelState
    fn get_channel_state(&self, did_hash: &str) -> Option<&ChatChannelState>;
    /// Get a mutable reference to the ChatChannelState
    fn get_channel_state_mut(&mut self, did_hash: &str) -> Option<&mut ChatChannelState>;
    /// Remove a DID from the Channel State
    fn remove_channel_state(&mut self, did_hash: &str) -> Option<ChatChannelState>;
    /// Insert a ChatChannelState into the Channel State
    fn insert_channel_state(
        &mut self,
        did_hash: &str,
        state: ChatChannelState,
    ) -> Option<ChatChannelState>;
    /// Get the model if it exists
    fn get_model(&self) -> Option<&OllamaModel> {
        None
    }
}

impl ChannelState for OllamaModel {
    fn get_channel_state(&self, did_hash: &str) -> Option<&ChatChannelState> {
        self.channel_state.get(did_hash)
    }

    fn get_channel_state_mut(&mut self, did_hash: &str) -> Option<&mut ChatChannelState> {
        self.channel_state.get_mut(did_hash)
    }

    fn remove_channel_state(&mut self, did_hash: &str) -> Option<ChatChannelState> {
        self.channel_state.remove(did_hash)
    }

    fn insert_channel_state(
        &mut self,
        did_hash: &str,
        state: ChatChannelState,
    ) -> Option<ChatChannelState> {
        self.channel_state.insert(did_hash.into(), state)
    }

    fn get_model(&self) -> Option<&OllamaModel> {
        Some(self)
    }
}

impl ChannelState for ConciergeState {
    fn get_channel_state(&self, did_hash: &str) -> Option<&ChatChannelState> {
        self.channel_state.get(did_hash)
    }

    fn get_channel_state_mut(&mut self, did_hash: &str) -> Option<&mut ChatChannelState> {
        self.channel_state.get_mut(did_hash)
    }

    fn remove_channel_state(&mut self, did_hash: &str) -> Option<ChatChannelState> {
        self.channel_state.remove(did_hash)
    }

    fn insert_channel_state(
        &mut self,
        did_hash: &str,
        state: ChatChannelState,
    ) -> Option<ChatChannelState> {
        self.channel_state.insert(did_hash.into(), state)
    }
}
