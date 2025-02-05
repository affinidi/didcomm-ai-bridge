use anyhow::{anyhow, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Input, MultiSelect, Select};
use didcomm_ollama::{
    config::{Config, OllamaModel},
    create_did, DIDMethods,
};
use ollama_rs::Ollama;
use regex::Regex;

pub(crate) async fn run_setup_wizard() -> Result<Config> {
    println!();
    println!("{}", style("Running setup wizard").green());
    let mediator_did = get_mediator_did()?;
    let did_method = get_did_method()?;
    let mut config = Config {
        concierge_did: create_did(&did_method, &mediator_did)?,
        mediator_did,
        ..Default::default()
    };

    add_new_model(&mut config, &did_method).await?;

    Ok(config)
}

pub(crate) async fn add_new_model(config: &mut Config, did_method: &DIDMethods) -> Result<()> {
    let (address, port) = get_ollama_address()?;
    add_ollama_models(&address, port, config, did_method).await?;

    Ok(())
}

fn get_mediator_did() -> Result<String> {
    let mediator_did = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Mediator DID")
        .default("did:web:mediator-nlb.storm.ws:mediator:v1:.well-known".into())
        .validate_with({
            move |input: &String| -> Result<(), &str> {
                let re = Regex::new(r"did:\w*:\w*").unwrap();
                if re.is_match(input) {
                    Ok(())
                } else {
                    Err("Invalid DID format")
                }
            }
        })
        .interact_text()
        .unwrap();

    Ok(mediator_did)
}

/// Select the DID method to use for generating keys
fn get_did_method() -> Result<DIDMethods> {
    let selected = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("DID Method to use for generating keys")
        .default(1)
        .items(&["did:key", "did:peer"])
        .interact()
        .unwrap();

    if selected == 0 {
        Ok(DIDMethods::Key)
    } else {
        Ok(DIDMethods::Peer)
    }
}

/// Get the Ollama address from the user
/// http://localhost:11434
/// # Returns
/// * `Ok((String, u16))` - The address and port of the Ollama service
fn get_ollama_address() -> Result<(String, u16)> {
    let ollama_address_re = Regex::new(r"^(http:\/\/[^:]*):(\d+)$").unwrap();
    let validate_re = ollama_address_re.clone();
    let ollama_address: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Ollama Service Address")
        .default("http://localhost:11434".into())
        .validate_with({
            move |input: &String| -> Result<(), &str> {
                match validate_re.captures(input) {
                    None => Err("This is not a valid address; must look similar to http://localhost:11434"),
                    Some(caps) => {
                        if caps.len() == 3 {
                        Ok(())
                        } else {
                        Err("This is not a valid address; must look similar to http://localhost:11434")
                }
            }
        }
        }})
        .interact_text()
        .unwrap();

    match ollama_address_re.captures(&ollama_address) {
        None => Err(anyhow::anyhow!(
            "This is not a valid address; must look similar to http://localhost:11434"
        )),
        Some(caps) => {
            if caps.len() != 3 {
                return Err(anyhow::anyhow!(
                    "This is not a valid address; must look similar to http://localhost:11434"
                ));
            }
            let port = caps.get(2).unwrap().as_str().parse::<u16>()?;
            Ok((caps.get(1).unwrap().as_str().to_string(), port))
        }
    }
}

/// Creates a list of Ollama models that you can select to enable
pub async fn add_ollama_models(
    host: &str,
    port: u16,
    config: &mut Config,
    did_method: &DIDMethods,
) -> Result<()> {
    let ollama = Ollama::new(host.to_string(), port);

    println!();
    let multi_select = ollama
        .list_local_models()
        .await
        .map_err(|e| anyhow!(format!("list_local_models() failed: {}", e.to_string())))?
        .iter()
        .map(|m| m.name.clone())
        .collect::<Vec<String>>();

    let mut defaults: Vec<bool> = Vec::new();
    for model in &multi_select {
        defaults.push(config.models.contains_key(model));
    }

    let selected = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Models to enable? (space to select, enter to confirm)")
        .items(&multi_select[..])
        .defaults(&defaults[..])
        .report(true)
        .interact()
        .unwrap();

    for s in &selected {
        config.add_model(
            &multi_select[*s],
            OllamaModel::new(
                host.to_string(),
                port,
                &config.mediator_did,
                &multi_select[*s],
                did_method,
            )?,
        );
    }

    // Check for what we removed
    for (i, e) in defaults.iter().enumerate() {
        if e == &true && !selected.contains(&i) {
            // Removing model
            println!("Removing Model {}", multi_select[i]);
            config.remove_model(&multi_select[i]);
        }
    }

    Ok(())
}
