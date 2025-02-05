/*!
 * Configuration information relating to DIDComm-Ollama bridge
 */

use anyhow::{Context, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs};

use crate::{create_did, DIDMethods};

/// Configuration for the DIDComm-Ollama bridge
#[derive(Default, Deserialize, Serialize)]
pub struct Config {
    /// Ollama models that have been configured
    pub models: HashMap<String, OllamaModel>,
    /// Mediator DID for DIDComm
    pub mediator_did: String,
    /// Concierge Agent DID for DIDComm
    pub concierge_did: String,
}

impl Config {
    /// Load the configuration from the specified file
    pub fn load(config_file: &str) -> Result<Config> {
        let contents = fs::read_to_string(config_file).context(format!(
            "Couldn't open configuration file ({})",
            config_file
        ))?;

        serde_json::from_str(&contents)
            .map_err(anyhow::Error::msg)
            .context(format!(
                "Parse error on configuration file ({})",
                config_file
            ))
    }

    /// Save the configuration to the specified file
    pub fn save(&self, config_file: &str) -> Result<()> {
        let contents =
            serde_json::to_string_pretty(&self).context("Couldn't serialize configuration")?;
        fs::write(config_file, contents).context(format!(
            "Couldn't write configuration file ({})",
            config_file
        ))
    }

    /// Add a Ollama model to the configuration
    pub fn add_model(&mut self, name: &str, model: OllamaModel) {
        self.models.insert(name.into(), model);
    }

    /// Remove a Ollama model from the configuration
    pub fn remove_model(&mut self, model_name: &str) {
        if let Some(model) = self.models.remove(model_name) {
            // Clean up secret keys
            let event = Entry::new("didcomm-ollama", &model.did).unwrap();
            let _ = event.delete_credential();
        }
    }
}

// ************************************************************************************************
//  OllamaModel
// ************************************************************************************************

/// OllamaModel represents a model within the Ollama Service
#[derive(Clone, Deserialize, Serialize)]
pub struct OllamaModel {
    /// Name of the model in Ollama
    pub name: String,
    /// Hostname of the Ollama service for this model
    pub ollama_host: String,
    /// Port of the Ollama service for this model
    pub ollama_port: u16,
    /// DID for sending messages to this model
    pub did: String,
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
            did: create_did(did_method, mediator_did)?,
        })
    }
}
