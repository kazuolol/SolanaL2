//! HTTP JSON-RPC Server
//!
//! Provides HTTP endpoint for JSON-RPC methods.

use crate::methods::{
    handle_get_account_info, handle_get_health, handle_get_latest_blockhash, handle_get_slot,
    handle_send_transaction, GetAccountInfoRequest, RpcContext, RpcError, SendTransactionRequest,
};
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
    tracing::info!("RPC method called: {}", method);
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
