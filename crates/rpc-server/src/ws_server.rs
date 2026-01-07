//! WebSocket Server
//!
//! Provides WebSocket endpoint for subscriptions.

use crate::{
    methods::RpcContext,
    subscriptions::{AccountNotification, SubscriptionManager},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solana_sdk::{account::ReadableAccount, pubkey::Pubkey};
use std::{str::FromStr, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::Message};

/// WebSocket JSON-RPC request
#[derive(Debug, Deserialize)]
pub struct WsJsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// WebSocket Server
pub struct WebSocketServer {
    context: Arc<RpcContext>,
    subscription_manager: Arc<SubscriptionManager>,
}

impl WebSocketServer {
    /// Create a new WebSocket server
    pub fn new(context: Arc<RpcContext>, subscription_manager: Arc<SubscriptionManager>) -> Self {
        Self {
            context,
            subscription_manager,
        }
    }

    /// Run the WebSocket server
    pub async fn run(self, addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!("WebSocket server listening on {}", addr);

        let context = self.context;
        let subscription_manager = self.subscription_manager;

        while let Ok((stream, peer_addr)) = listener.accept().await {
            let ctx = context.clone();
            let sub_mgr = subscription_manager.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, ctx, sub_mgr).await {
                    tracing::warn!("WebSocket connection error from {}: {}", peer_addr, e);
                }
            });
        }

        Ok(())
    }
}

/// Handle a single WebSocket connection
async fn handle_connection(
    stream: TcpStream,
    context: Arc<RpcContext>,
    subscription_manager: Arc<SubscriptionManager>,
) -> anyhow::Result<()> {
    let ws_stream = accept_async(stream).await?;
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Track subscriptions for this connection
    let mut active_subscriptions: Vec<u64> = Vec::new();

    while let Some(msg) = ws_receiver.next().await {
        let msg = msg?;

        if let Message::Text(text) = msg {
            let request: WsJsonRpcRequest = match serde_json::from_str(&text) {
                Ok(req) => req,
                Err(_) => continue,
            };

            let response = handle_ws_method(
                &context,
                &subscription_manager,
                &request,
                &mut active_subscriptions,
                &mut ws_sender,
            )
            .await;

            let response_json = serde_json::to_string(&response)?;
            ws_sender.send(Message::Text(response_json)).await?;
        }
    }

    // Clean up subscriptions on disconnect
    for sub_id in active_subscriptions {
        subscription_manager.unsubscribe(sub_id);
    }

    Ok(())
}

/// Handle WebSocket JSON-RPC method
async fn handle_ws_method<S>(
    context: &RpcContext,
    subscription_manager: &SubscriptionManager,
    request: &WsJsonRpcRequest,
    active_subscriptions: &mut Vec<u64>,
    ws_sender: &mut S,
) -> Value
where
    S: SinkExt<Message> + Unpin,
{
    match request.method.as_str() {
        "accountSubscribe" => {
            let params: Vec<Value> = serde_json::from_value(request.params.clone()).unwrap_or_default();
            let pubkey_str = params.first().and_then(|v| v.as_str());

            match pubkey_str {
                Some(pk_str) => match Pubkey::from_str(pk_str) {
                    Ok(pubkey) => {
                        let (sub_id, mut receiver) = subscription_manager.subscribe_account(pubkey);
                        active_subscriptions.push(sub_id);

                        // Spawn task to forward notifications
                        let sub_id_clone = sub_id;
                        tokio::spawn(async move {
                            while let Ok(notification) = receiver.recv().await {
                                // Format and send notification
                                // This is simplified - in production would use proper channel
                                tracing::debug!(
                                    "Account notification for sub {}: {}",
                                    sub_id_clone,
                                    notification.pubkey
                                );
                            }
                        });

                        json!({
                            "jsonrpc": "2.0",
                            "id": request.id,
                            "result": sub_id
                        })
                    }
                    Err(_) => error_response(&request.id, -32602, "Invalid pubkey"),
                },
                None => error_response(&request.id, -32602, "Missing pubkey parameter"),
            }
        }

        "accountUnsubscribe" => {
            let params: Vec<Value> = serde_json::from_value(request.params.clone()).unwrap_or_default();
            let sub_id = params.first().and_then(|v| v.as_u64());

            match sub_id {
                Some(id) => {
                    let success = subscription_manager.unsubscribe(id);
                    if success {
                        active_subscriptions.retain(|&s| s != id);
                    }
                    json!({
                        "jsonrpc": "2.0",
                        "id": request.id,
                        "result": success
                    })
                }
                None => error_response(&request.id, -32602, "Missing subscription ID"),
            }
        }

        _ => error_response(&request.id, -32601, &format!("Method not found: {}", request.method)),
    }
}

/// Create error response
fn error_response(id: &Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

/// Format account notification for WebSocket
pub fn format_account_notification(notification: &AccountNotification) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "accountNotification",
        "params": {
            "result": {
                "context": {
                    "slot": notification.slot
                },
                "value": {
                    "data": [BASE64.encode(notification.account.data()), "base64"],
                    "executable": notification.account.executable(),
                    "lamports": notification.account.lamports(),
                    "owner": notification.account.owner().to_string(),
                    "rentEpoch": 0
                }
            },
            "subscription": notification.subscription_id
        }
    })
}
