/*!
 * Processing of chat messages
 */

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

use crate::config::OllamaModel;

#[derive(Deserialize, Serialize)]
struct ChatMessage {
    pub text: String,
}

/// Processes a received message
/// Doesn't return anything
pub(crate) async fn handle_message(
    atm: &ATM,
    profile: &Arc<Profile>,
    model: &OllamaModel,
    message: &Message,
) {
    let Some(from_did) = message.from.clone() else {
        println!("{}", style("No 'from' field in message").red());
        println!(
            "{}",
            style("How would one respond to an anonymous message?").red()
        );
        let _ = atm.delete_message_background(profile, &message.id).await;
        return;
    };

    let Ok(msg_type) = MessageType::from_str(&message.type_) else {
        println!(
            "{}",
            style(format!("Unknown message type: {:?}", message)).red()
        );
        let _ = atm.delete_message_background(profile, &message.id).await;
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
                        let _ = handle_prompt(atm, profile, &chat_message, model, &from_did).await;
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
    let _ = atm.delete_message_background(profile, &message.id).await;
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
                    let _ = send_message(atm, profile, &output, from_did).await;
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

    let _ = send_message(atm, profile, &output, from_did).await;
    println!("{}", style("AI Responded...").cyan());

    Ok(())
}

pub async fn send_message(
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

    if packed.1.messaging_service.is_none() {
        let _ = atm
            .forward_and_send_message(
                profile,
                &packed.0,
                None,
                profile.dids()?.1,
                from_did,
                None,
                None,
                false,
            )
            .await?;
    } else {
        let _ = atm
            .send_message(profile, &packed.0, &id, false, false)
            .await?;
    }
    Ok(())
}
