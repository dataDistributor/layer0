use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use dxid_ai_hypervisor::Hypervisor;
use dxid_config::DxidConfig;
use dxid_core::Address;
use dxid_crypto::address_from_string;
use dxid_storage::{BlockStore, PgStore, StateStore};
use serde::{Deserialize, Serialize};
use tonic::{transport::Server, Request, Response, Status};
use tracing::info;

pub mod proto {
    tonic::include_proto!("dxid");
}

#[derive(Clone)]
pub struct RpcState {
    pub store: Arc<PgStore>,
    pub hypervisor: Arc<Hypervisor>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct StatusResponse {
    height: u64,
    peers: usize,
}

pub async fn start_servers(cfg: &DxidConfig, store: Arc<PgStore>, hypervisor: Arc<Hypervisor>) -> Result<()> {
    let state = RpcState { store, hypervisor };
    let rest_addr: SocketAddr = cfg.api.rest_addr.parse()?;
    let grpc_addr: SocketAddr = cfg.api.grpc_addr.parse()?;
    let rest_handle = tokio::spawn(run_rest(rest_addr, state.clone()));
    let grpc_handle = tokio::spawn(run_grpc(grpc_addr, state));
    rest_handle.await??;
    grpc_handle.await??;
    Ok(())
}

async fn run_rest(addr: SocketAddr, state: RpcState) -> Result<()> {
    let app = Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/blocks/:height", get(get_block))
        .route("/balance/:address", get(balance))
        .route("/ai/query", post(ai_query))
        .with_state(state);
    info!("REST listening on {addr}");
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn status(State(state): State<RpcState>) -> Json<StatusResponse> {
    // Height derived from block count for demo purposes.
    let height = state
        .store
        .get_block_by_height(0)
        .await
        .ok()
        .flatten()
        .map(|b| b.header.height)
        .unwrap_or(0);
    Json(StatusResponse { height, peers: 0 })
}

async fn get_block(
    State(state): State<RpcState>,
    Path(height): Path<u64>,
) -> Result<Json<serde_json::Value>, Status> {
    let block = state
        .store
        .get_block_by_height(height as i64)
        .await
        .map_err(|_| Status::internal("db error"))?;
    Ok(Json(serde_json::json!({ "block": block })))
}

async fn balance(
    State(state): State<RpcState>,
    Path(addr): Path<String>,
) -> Result<Json<serde_json::Value>, Status> {
    let address = address_from_string(&addr).map_err(|_| Status::invalid_argument("bad address"))?;
    let balance = state
        .store
        .get_balance(&address)
        .await
        .map_err(|_| Status::internal("db error"))?;
    Ok(Json(serde_json::json!({ "balance": balance })))
}

#[derive(Deserialize)]
struct AiRequest {
    prompt: String,
}

async fn ai_query(
    State(state): State<RpcState>,
    Json(req): Json<AiRequest>,
) -> Result<Json<serde_json::Value>, Status> {
    let response = state
        .hypervisor
        .query(&req.prompt)
        .await
        .map_err(|_| Status::internal("ai error"))?;
    Ok(Json(serde_json::json!({ "answer": response })))
}

#[derive(Clone)]
pub struct GrpcService {
    state: RpcState,
}

#[tonic::async_trait]
impl proto::dxid_server::Dxid for GrpcService {
    async fn get_status(
        &self,
        _request: Request<proto::StatusRequest>,
    ) -> Result<Response<proto::StatusResponse>, Status> {
        let height = self
            .state
            .store
            .get_block_by_height(0)
            .await
            .ok()
            .flatten()
            .map(|b| b.header.height)
            .unwrap_or(0);
        let reply = proto::StatusResponse {
            height,
            peers: 0,
            version: "0.1.0".into(),
        };
        Ok(Response::new(reply))
    }

    async fn get_block(
        &self,
        request: Request<proto::BlockRequest>,
    ) -> Result<Response<proto::BlockResponse>, Status> {
        let height = request.into_inner().height;
        let block = self
            .state
            .store
            .get_block_by_height(height as i64)
            .await
            .map_err(|_| Status::internal("db error"))?;
        let json = serde_json::to_string(&block).unwrap_or_default();
        Ok(Response::new(proto::BlockResponse { block_json: json }))
    }

    async fn get_balance(
        &self,
        request: Request<proto::BalanceRequest>,
    ) -> Result<Response<proto::BalanceResponse>, Status> {
        let addr = request.into_inner().address;
        let address = address_from_string(&addr).map_err(|_| Status::invalid_argument("bad address"))?;
        let balance = self
            .state
            .store
            .get_balance(&address)
            .await
            .map_err(|_| Status::internal("db error"))?;
        Ok(Response::new(proto::BalanceResponse { balance }))
    }

    async fn ai_query(
        &self,
        request: Request<proto::AiQueryRequest>,
    ) -> Result<Response<proto::AiQueryResponse>, Status> {
        let prompt = request.into_inner().prompt;
        let answer = self
            .state
            .hypervisor
            .query(&prompt)
            .await
            .map_err(|_| Status::internal("ai error"))?;
        Ok(Response::new(proto::AiQueryResponse { answer }))
    }
}

async fn run_grpc(addr: SocketAddr, state: RpcState) -> Result<()> {
    info!("gRPC listening on {addr}");
    let svc = GrpcService { state };
    Server::builder()
        .add_service(proto::dxid_server::DxidServer::new(svc))
        .serve(addr)
        .await?;
    Ok(())
}
