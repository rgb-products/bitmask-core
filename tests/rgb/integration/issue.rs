#![cfg(not(target_arch = "wasm32"))]
use crate::rgb::integration::utils::{get_collectible_data, get_uda_data, issuer_issue_contract};

#[tokio::test]
async fn allow_issuer_issue_fungible_contract() -> anyhow::Result<()> {
    let issuer_resp = issuer_issue_contract("RGB20", 5, false, None).await;
    assert!(issuer_resp.is_ok());
    Ok(())
}

#[tokio::test]
async fn allow_issuer_issue_uda_contract() -> anyhow::Result<()> {
    let single = Some(get_uda_data());
    let issuer_resp = issuer_issue_contract("RGB21", 1, false, single).await;
    assert!(issuer_resp.is_ok());
    Ok(())
}

#[tokio::test]
async fn allow_issuer_issue_collectible_contract() -> anyhow::Result<()> {
    let collectible = Some(get_collectible_data());
    let issuer_resp = issuer_issue_contract("RGB21", 1, false, collectible).await;
    assert!(issuer_resp.is_ok());
    Ok(())
}
