#![cfg(feature = "server")]
use std::{env, net::SocketAddr};

use anyhow::Result;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use bitmask_core::{
    accept_transfer, create_invoice, create_psbt,
    data::structs::{
        AcceptRequest, AssetRequest, BlindRequest, InvoiceRequest, IssueRequest, PsbtRequest,
        RgbTransferRequest, TransfersRequest,
    },
    get_blinded_utxo, import_asset, issue_contract, pay_asset, transfer_assets,
};
use log::info;
use tower_http::cors::CorsLayer;

async fn issue(Json(issue): Json<IssueRequest>) -> Result<impl IntoResponse, AppError> {
    let issue_res = issue_contract(
        &issue.ticker,
        &issue.name,
        &issue.description,
        issue.precision,
        issue.supply,
        &issue.seal,
        &issue.iface,
    )
    .await?;

    Ok((StatusCode::OK, Json(issue_res)))
}

async fn invoice(Json(invoice): Json<InvoiceRequest>) -> Result<impl IntoResponse, AppError> {
    let invoice_res = create_invoice(
        &invoice.contract_id,
        &invoice.iface,
        invoice.amount,
        &invoice.seal,
    )
    .await?;

    Ok((StatusCode::OK, Json(invoice_res)))
}

async fn psbt(Json(psbt_req): Json<PsbtRequest>) -> Result<impl IntoResponse, AppError> {
    let psbt_res = create_psbt(psbt_req).await?;
    Ok((StatusCode::OK, Json(psbt_res)))
}

#[axum_macros::debug_handler]
async fn pay(Json(pay_req): Json<RgbTransferRequest>) -> Result<impl IntoResponse, AppError> {
    let transfer_res = pay_asset(pay_req).await?;

    Ok((StatusCode::OK, Json(transfer_res)))
}

async fn blind(Json(blind): Json<BlindRequest>) -> Result<impl IntoResponse, AppError> {
    let blind_res = get_blinded_utxo(&blind.utxo)?;

    Ok((StatusCode::OK, Json(blind_res)))
}

async fn import(Json(asset): Json<AssetRequest>) -> Result<impl IntoResponse, AppError> {
    let asset_res = import_asset(&asset.asset, asset.utxos)?;

    Ok((StatusCode::OK, Json(asset_res)))
}

#[axum_macros::debug_handler]
async fn _transfer(Json(transfer): Json<TransfersRequest>) -> Result<impl IntoResponse, AppError> {
    let transfer_res = transfer_assets(transfer).await?;

    Ok((StatusCode::OK, Json(transfer_res)))
}

async fn accept(Json(accept): Json<AcceptRequest>) -> Result<impl IntoResponse, AppError> {
    accept_transfer(
        &accept.consignment,
        &accept.blinding_factor,
        &accept.outpoint,
    )
    .await?;

    Ok(StatusCode::OK)
}

#[tokio::main]
async fn main() -> Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug");
    }

    pretty_env_logger::init();

    let app = Router::new()
        .route("/issue", post(issue))
        .route("/invoice", post(invoice))
        .route("/psbt", post(psbt))
        .route("/pay", post(pay))
        .route("/import", post(import))
        .route("/blind", post(blind))
        .route("/accept", post(accept))
        .layer(CorsLayer::permissive());

    let addr = SocketAddr::from(([127, 0, 0, 1], 7070));

    info!("bitmaskd REST server successfully running at {addr}");

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

// https://github.com/tokio-rs/axum/blob/fef95bf37a138cdf94985e17f27fd36481525171/examples/anyhow-error-response/src/main.rs
// Make our own error that wraps `anyhow::Error`.
struct AppError(anyhow::Error);

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self.0),
        )
            .into_response()
    }
}

// This enables using `?` on functions that return `Result<_, anyhow::Error>` to turn them into
// `Result<_, AppError>`. That way you don't need to do that manually.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
