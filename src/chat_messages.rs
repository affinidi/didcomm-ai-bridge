/*!
 * Processing of chat messages
 */

use crate::{config::OllamaModel, Config};
use affinidi_messaging_didcomm::Message;
use affinidi_messaging_sdk::{
    messages::known::MessageType, profiles::Profile,
    protocols::message_pickup::MessagePickupStatusReply, ATM,
};
use anyhow::Result;
use console::style;
use ollama_rs::{generation::completion::request::GenerationRequest, Ollama};
use serde::{Deserialize, Serialize};
use std::{str::FromStr, sync::Arc};
use tokio::io::{stdout, AsyncWriteExt};
use tokio_stream::StreamExt;

#[derive(Deserialize, Serialize)]
struct ChatMessage {
    pub text: String,
}

/// Processes a received message
/// Doesn't return anything
pub(crate) async fn handle_message(config: &Config, atm: &ATM, message: &Message) {
    // Get the profile of the recipient
    let profile = {
        let lock = atm.get_profiles();
        let lock = lock.read().await;
        let Some(to_dids) = message.to.as_ref() else {
            println!("{}", style("No 'to' field in message").red());
            return;
        };
        let Some(to_did) = to_dids.first() else {
            println!(
                "{}",
                style(format!(
                    "To field exists, but there are no DID's present: {:?}",
                    message.to
                ))
                .red()
            );
            return;
        };
        if let Some(profile) = lock.find_by_did(to_did) {
            profile
        } else {
            println!(
                "{}",
                style(format!("No profile found for DID: {}", to_did)).red()
            );
            return;
        }
    };

    let Some(from_did) = message.from.clone() else {
        println!("{}", style("No 'from' field in message").red());
        println!(
            "{}",
            style("How would one respond to an anonymous message?").red()
        );
        let _ = atm.delete_message_background(&profile, &message.id).await;
        return;
    };

    // Get the model associated with this DIDComm profile
    let Some(model) = config.models.get(profile.inner.alias.as_str()) else {
        println!(
            "{}",
            style(format!(
                "No model found for profile: {:?}",
                profile.inner.alias
            ))
            .red()
        );
        return;
    };

    let Ok(msg_type) = MessageType::from_str(&message.type_) else {
        println!(
            "{}",
            style(format!("Unknown message type: {:?}", message)).red()
        );
        let _ = atm.delete_message_background(&profile, &message.id).await;
        return;
    };

    match msg_type {
        MessageType::MessagePickupStatusResponse => {
            match serde_json::from_value::<MessagePickupStatusReply>(message.body.clone()) {
                Ok(status) => {
                    println!(
                        "{}",
                        style(format!(
                            "STATUS: queued messages ({}), live_streaming?({})",
                            status.message_count, status.live_delivery
                        ))
                        .green()
                    );
                }
                Err(e) => {
                    println!(
                        "{}",
                        style(format!("Error parsing message body: {:?}", e)).red()
                    );
                    return;
                }
            }
        }
        MessageType::Other(_type) => match _type.as_str() {
            "https://affinidi.com/atm/client-actions/chat-message" => {
                match serde_json::from_value::<ChatMessage>(message.body.clone()) {
                    Ok(chat_message) => {
                        println!(
                            "{}",
                            style(format!(
                                "Model ({}): incoming prompt: {:?}",
                                model.name, chat_message.text
                            ))
                            .green()
                        );
                        let _ = handle_prompt(atm, &profile, &chat_message, model, &from_did).await;
                    }
                    Err(e) => {
                        println!(
                            "{}",
                            style(format!("Error parsing chat message: {:?}", e)).red()
                        );
                        return;
                    }
                }
            }
            _ => {
                println!(
                    "{}",
                    style(format!("Unknown Message Type: {} received!", _type)).red()
                );
            }
        },
        _ => {
            println!("Received message: {:?}", message);
        }
    }
    let _ = atm.delete_message_background(&profile, &message.id).await;
}

/// Handles a prompt message
async fn handle_prompt(
    atm: &ATM,
    profile: &Arc<Profile>,
    chat_message: &ChatMessage,
    model: &OllamaModel,
    from_did: &str,
) -> Result<()> {
    // Instantiate Ollama
    let ollama = Ollama::new(&model.ollama_host, model.ollama_port);

    let mut stream = ollama
        .generate_stream(GenerationRequest::new(
            model.name.clone(),
            chat_message.text.clone(),
        ))
        .await
        .unwrap();

    let mut stdout = stdout();
    stdout.write_all(b"\n> ").await?;
    stdout.flush().await?;

    let mut think_flag = true;
    let mut output = String::new();
    while let Some(Ok(res)) = stream.next().await {
        for ele in res {
            //stdout.write_all(ele.response.as_bytes()).await?;
            if !think_flag {
                if ele.response == "\n\n" {
                    continue;
                } else if ele.response == ".\n\n" {
                    output.push_str(&ele.response);
                    let _ = _send_message(atm, profile, &output, from_did).await;
                    output.clear();

                    continue;
                }
                //println!("{:?}", ele);
                output.push_str(&ele.response);
            }
            if ele.response.contains("</think>") {
                think_flag = false;
            }

            stdout.flush().await?;
        }
    }
    let _ = _send_message(atm, profile, &output, from_did).await;
    println!("{}", style("AI Responded...").cyan());

    Ok(())
}

async fn _send_message(
    atm: &ATM,
    profile: &Arc<Profile>,
    text: &str,
    from_did: &str,
) -> Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let msg = Message::build(
        id.clone(),
        "https://affinidi.com/atm/client-actions/chat-message".to_string(),
        serde_json::json!({ "text": text }),
    )
    .from(profile.inner.did.clone())
    .to(from_did.to_string())
    .finalize();

    let packed = atm
        .pack_encrypted(
            &msg,
            from_did,
            Some(&profile.inner.did),
            Some(&profile.inner.did),
        )
        .await?;

    let _ = atm
        .send_message(profile, &packed.0, &id, false, false)
        .await?;
    Ok(())
}
