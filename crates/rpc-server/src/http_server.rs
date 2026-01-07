//! HTTP JSON-RPC Server
//!
//! Provides HTTP endpoint for JSON-RPC methods.

use crate::methods::{
    handle_get_account_info, handle_get_health, handle_get_latest_blockhash, handle_get_slot,
    handle_send_transaction, GetAccountInfoRequest, RpcContext, RpcError, SendTransactionRequest,
};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use axum::{
    extract::State,
    http::{header, Method, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

/// JSON-RPC request
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// JSON-RPC response
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

/// HTTP RPC Server
pub struct HttpRpcServer {
    context: Arc<RpcContext>,
}

impl HttpRpcServer {
    /// Create a new HTTP RPC server
    pub fn new(context: Arc<RpcContext>) -> Self {
        Self { context }
    }

    /// Create the Axum router
    pub fn router(self) -> Router {
        // CORS layer to allow browser clients
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([header::CONTENT_TYPE, header::ACCEPT]);

        Router::new()
            .route("/", post(handle_rpc))
            .layer(cors)
            .with_state(self.context)
    }

    /// Run the server
    pub async fn run(self, addr: &str) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("HTTP RPC server listening on {}", addr);

        axum::serve(listener, self.router()).await?;
        Ok(())
    }
}

/// Handle JSON-RPC request
async fn handle_rpc(
    State(context): State<Arc<RpcContext>>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    let result = dispatch_method(&context, &request.method, request.params);

    let response = match result {
        Ok(value) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(value),
            error: None,
        },
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: error_code(&e),
                message: e.to_string(),
            }),
        },
    };

    (StatusCode::OK, Json(response))
}

/// Dispatch to appropriate method handler
fn dispatch_method(ctx: &RpcContext, method: &str, params: Value) -> Result<Value, RpcError> {
    match method {
        "sendTransaction" => {
            let params: Vec<Value> = serde_json::from_value(params).unwrap_or_default();
            let transaction = params
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::InvalidParams("Missing transaction".to_string()))?;

            let encoding = params.get(1).and_then(|v| v.as_str()).map(String::from);

            let request = SendTransactionRequest {
                transaction: transaction.to_string(),
                encoding,
            };

            let sig = handle_send_transaction(ctx, request)?;
            Ok(json!(sig))
        }

        "getAccountInfo" => {
            let params: Vec<Value> = serde_json::from_value(params).unwrap_or_default();
            let pubkey = params
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::InvalidParams("Missing pubkey".to_string()))?;

            let encoding = params
                .get(1)
                .and_then(|v| v.get("encoding"))
                .and_then(|v| v.as_str())
                .map(String::from);

            let request = GetAccountInfoRequest {
                pubkey: pubkey.to_string(),
                encoding,
            };

            let response = handle_get_account_info(ctx, request)?;
            Ok(serde_json::to_value(response).unwrap())
        }

        "getLatestBlockhash" => {
            let response = handle_get_latest_blockhash(ctx)?;
            Ok(serde_json::to_value(response).unwrap())
        }

        "getSlot" => {
            let slot = handle_get_slot(ctx)?;
            Ok(json!(slot))
        }

        "getHealth" => {
            let health = handle_get_health()?;
            Ok(json!(health))
        }

        "getVersion" => Ok(json!({
            "solana-core": "2.1.0",
            "feature-set": 0,
            "l2-version": env!("CARGO_PKG_VERSION"),
        })),

        // ============ Game Methods (MVP bypass of full SVM) ============

        "game_joinWorld" => {
            let params: Vec<Value> = serde_json::from_value(params).unwrap_or_default();
            let authority = params
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::InvalidParams("Missing authority pubkey".to_string()))?;
            let player_name = params
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("Player");

            let authority_pubkey = Pubkey::from_str(authority)
                .map_err(|_| RpcError::InvalidParams("Invalid authority pubkey".to_string()))?;

            let slot = *ctx.current_slot.read();
            let player_pda = ctx.game_handler
                .join_world(authority_pubkey, player_name, slot)
                .map_err(|e| RpcError::InternalError(e))?;

            // Generate a pseudo-tx signature for logging
            let tx_sig = format!("{:x}{:x}{:08x}",
                player_pda.to_bytes()[0..4].iter().fold(0u32, |a, &b| a.wrapping_add(b as u32)),
                slot,
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()
            );

            Ok(json!({
                "playerPda": player_pda.to_string(),
                "slot": slot,
                "signature": tx_sig,
                "action": "joinWorld"
            }))
        }

        "game_move" => {
            let params: Vec<Value> = serde_json::from_value(params).unwrap_or_default();
            let authority = params
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::InvalidParams("Missing authority pubkey".to_string()))?;
            let direction = params
                .get(1)
                .and_then(|v| v.as_u64())
                .unwrap_or(255) as u8;
            let sprint = params
                .get(2)
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let authority_pubkey = Pubkey::from_str(authority)
                .map_err(|_| RpcError::InvalidParams("Invalid authority pubkey".to_string()))?;

            let slot = *ctx.current_slot.read();
            ctx.game_handler
                .move_player(authority_pubkey, direction, sprint, slot)
                .map_err(|e| RpcError::InternalError(e))?;

            Ok(json!({ "success": true, "slot": slot }))
        }

        "game_move3d" => {
            let params: Vec<Value> = serde_json::from_value(params).unwrap_or_default();
            let authority = params
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::InvalidParams("Missing authority pubkey".to_string()))?;
            let move_x = params
                .get(1)
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i8;
            let move_z = params
                .get(2)
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i8;
            let camera_yaw = params
                .get(3)
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i16;
            let sprint = params
                .get(4)
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let jump = params
                .get(5)
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let authority_pubkey = Pubkey::from_str(authority)
                .map_err(|_| RpcError::InvalidParams("Invalid authority pubkey".to_string()))?;

            let input = crate::game_handler::MovementInput3D {
                move_x,
                move_z,
                camera_yaw,
                sprint,
                jump,
            };

            let slot = *ctx.current_slot.read();
            let player_pda = ctx.game_handler
                .move_player_3d(authority_pubkey, input, slot)
                .map_err(|e| RpcError::InternalError(e))?;

            // Generate a pseudo-tx signature for logging
            let tx_sig = format!("{:x}{:x}{:08x}",
                player_pda.to_bytes()[0..4].iter().fold(0u32, |a, &b| a.wrapping_add(b as u32)),
                slot,
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()
            );

            Ok(json!({
                "success": true,
                "slot": slot,
                "signature": tx_sig,
                "account": player_pda.to_string(),
                "action": "move3d"
            }))
        }

        "game_getPlayer" => {
            let params: Vec<Value> = serde_json::from_value(params).unwrap_or_default();
            let authority = params
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::InvalidParams("Missing authority pubkey".to_string()))?;

            let authority_pubkey = Pubkey::from_str(authority)
                .map_err(|_| RpcError::InvalidParams("Invalid authority pubkey".to_string()))?;

            match ctx.game_handler.get_player(&authority_pubkey) {
                Some(player) => Ok(json!({
                    "authority": player.authority.to_string(),
                    "world": player.world.to_string(),
                    "positionX": player.position_x,
                    "positionZ": player.position_z,
                    "positionY": player.position_y,
                    "velocityX": player.velocity_x,
                    "velocityZ": player.velocity_z,
                    "velocityY": player.velocity_y,
                    "yaw": player.yaw,
                    "isGrounded": player.is_grounded,
                    "health": player.health,
                    "maxHealth": player.max_health,
                    "name": String::from_utf8_lossy(&player.name).trim_end_matches('\0').to_string()
                })),
                None => Ok(json!(null))
            }
        }

        "game_getAllPlayers" => {
            let players: Vec<Value> = ctx.game_handler.get_all_players()
                .into_iter()
                .map(|(pda, player)| json!({
                    "pda": pda.to_string(),
                    "authority": player.authority.to_string(),
                    "positionX": player.position_x,
                    "positionZ": player.position_z,
                    "positionY": player.position_y,
                    "yaw": player.yaw,
                    "health": player.health,
                    "maxHealth": player.max_health,
                    "name": String::from_utf8_lossy(&player.name).trim_end_matches('\0').to_string()
                }))
                .collect();
            Ok(json!(players))
        }

        _ => Err(RpcError::MethodNotFound(method.to_string())),
    }
}

/// Map error to JSON-RPC error code
fn error_code(error: &RpcError) -> i32 {
    match error {
        RpcError::InvalidParams(_) => -32602,
        RpcError::MethodNotFound(_) => -32601,
        RpcError::InternalError(_) => -32603,
    }
}
