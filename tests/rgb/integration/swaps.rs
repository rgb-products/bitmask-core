#![allow(unused_imports)]
#![cfg(not(target_arch = "wasm32"))]
use crate::rgb::integration::utils::{
    generate_new_block, get_uda_data, issuer_issue_contract_v2, send_some_coins, UtxoFilter,
};
use bitmask_core::{
    bitcoin::{
        fund_vault, get_new_address, get_wallet, new_mnemonic, publish_psbt_file,
        sign_and_publish_psbt_file, sign_psbt_file, sync_wallet,
    },
    rgb::{
        accept_transfer, create_auction_bid, create_auction_offers, create_buyer_bid,
        create_seller_offer, create_swap_transfer, create_watcher, finish_auction_offers,
        get_contract, import as import_contract, structs::ContractAmount, swap::RgbSwapStrategy,
        update_seller_offer, verify_transfers,
    },
    structs::{
        AcceptRequest, AssetType, ImportRequest, IssueResponse, PsbtFeeRequest, PublishPsbtRequest,
        RgbAuctionBidRequest, RgbAuctionOfferRequest, RgbAuctionOfferResponse, RgbBidRequest,
        RgbBidResponse, RgbOfferRequest, RgbOfferResponse, RgbOfferUpdateRequest, RgbSwapRequest,
        RgbSwapResponse, SecretString, SignPsbtRequest, SignedPsbtResponse, WatcherRequest,
    },
    util::init_logging,
};

#[tokio::test]
async fn create_hotswap_swap() -> anyhow::Result<()> {
    // 1. Initial Setup
    let seller_keys = new_mnemonic(&SecretString("".to_string())).await?;
    let buyer_keys = new_mnemonic(&SecretString("".to_string())).await?;

    let seller_sk = seller_keys.private.nostr_prv.clone();
    let watcher_name = "default";
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: seller_keys.public.watcher_xpub.clone(),
        force: false,
    };
    create_watcher(&seller_sk, create_watch_req.clone()).await?;

    let buyer_sk = buyer_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: buyer_keys.public.watcher_xpub.clone(),
        force: false,
    };
    create_watcher(&buyer_sk, create_watch_req.clone()).await?;

    // 2. Setup Wallets (Seller)
    let btc_address_1 = get_new_address(
        &SecretString(seller_keys.public.btc_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let default_coins = "0.001";
    send_some_coins(&btc_address_1, default_coins).await;

    let btc_descriptor_xprv = SecretString(seller_keys.private.btc_descriptor_xprv.clone());
    let btc_change_descriptor_xprv =
        SecretString(seller_keys.private.btc_change_descriptor_xprv.clone());

    let assets_address_1 = get_new_address(
        &SecretString(seller_keys.public.rgb_assets_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let uda_address_1 = get_new_address(
        &SecretString(seller_keys.public.rgb_udas_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let btc_wallet = get_wallet(&btc_descriptor_xprv, Some(&btc_change_descriptor_xprv)).await?;
    sync_wallet(&btc_wallet).await?;

    let fund_vault = fund_vault(
        &btc_descriptor_xprv,
        &btc_change_descriptor_xprv,
        &assets_address_1,
        &uda_address_1,
        Some(1.1),
    )
    .await?;

    // 3. Send some coins (Buyer)
    let btc_address_1 = get_new_address(
        &SecretString(buyer_keys.public.btc_descriptor_xpub.clone()),
        None,
    )
    .await?;
    let asset_address_1 = get_new_address(
        &SecretString(buyer_keys.public.rgb_assets_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let default_coins = "0.1";
    send_some_coins(&btc_address_1, default_coins).await;
    send_some_coins(&asset_address_1, default_coins).await;

    // 4. Issue Contract (Seller)
    let issuer_resp = issuer_issue_contract_v2(
        1,
        "RGB20",
        ContractAmount::with(5, 0, 2).to_value(),
        false,
        false,
        None,
        None,
        Some(UtxoFilter::with_outpoint(
            fund_vault.assets_output.unwrap_or_default(),
        )),
        Some(seller_keys.clone()),
    )
    .await?;

    let IssueResponse {
        contract_id,
        iface,
        supply,
        contract,
        precision,
        ..
    } = issuer_resp[0].clone();

    let buyer_import_req = ImportRequest {
        import: AssetType::RGB20,
        data: contract.strict,
    };
    let buyer_import_resp = import_contract(&buyer_sk, buyer_import_req).await;
    assert!(buyer_import_resp.is_ok());

    // 5. Create Seller Swap Side
    let contract_amount = supply - 1;
    let bitcoin_price: u64 = 100_000;
    let seller_asset_desc = seller_keys.public.rgb_assets_descriptor_xpub.clone();
    let expire_at = (chrono::Local::now() + chrono::Duration::minutes(5))
        .naive_utc()
        .timestamp();

    let asset_amount = ContractAmount::new(contract_amount, precision).to_string();
    let seller_swap_req = RgbOfferRequest {
        contract_id: contract_id.clone(),
        iface: iface.clone(),
        contract_amount: asset_amount.clone(),
        bitcoin_price,
        descriptor: SecretString(seller_asset_desc),
        change_terminal: "/20/1".to_string(),
        bitcoin_changes: vec![],
        strategy: RgbSwapStrategy::HotSwap,
        expire_at: Some(expire_at),
    };

    let seller_swap_resp = create_seller_offer(&seller_sk, seller_swap_req).await;
    assert!(seller_swap_resp.is_ok());

    // 7. Create Buyer Swap Side
    let RgbOfferResponse { offer_id, .. } = seller_swap_resp?;

    let bid_amount = "4.0";
    let buyer_btc_desc = buyer_keys.public.btc_descriptor_xpub.clone();
    let buyer_swap_req = RgbBidRequest {
        offer_id: offer_id.clone(),
        asset_amount: bid_amount.to_string(),
        descriptor: SecretString(buyer_btc_desc),
        change_terminal: "/1/0".to_string(),
        fee: PsbtFeeRequest::Value(1000),
    };

    let buyer_swap_resp = create_buyer_bid(&buyer_sk, buyer_swap_req).await;
    assert!(buyer_swap_resp.is_ok());

    // 8. Sign the Buyer Side
    let RgbBidResponse {
        bid_id, swap_psbt, ..
    } = buyer_swap_resp?;
    let request = SignPsbtRequest {
        psbt: swap_psbt,
        descriptors: vec![
            SecretString(buyer_keys.private.btc_descriptor_xprv.clone()),
            SecretString(buyer_keys.private.btc_change_descriptor_xprv.clone()),
        ],
    };
    let buyer_psbt_resp = sign_psbt_file(request).await;
    assert!(buyer_psbt_resp.is_ok());

    // 9. Create Swap PSBT
    let SignedPsbtResponse {
        psbt: swap_psbt, ..
    } = buyer_psbt_resp?;
    let final_swap_req = RgbSwapRequest {
        offer_id,
        bid_id,
        swap_psbt,
    };

    let final_swap_resp = create_swap_transfer(&seller_sk, final_swap_req).await;
    assert!(final_swap_resp.is_ok());

    // 10. Save Consig
    let RgbSwapResponse {
        final_psbt,
        consig_id,
        ..
    } = final_swap_resp?;

    // 11. Sign the Final PSBT
    let request = SignPsbtRequest {
        psbt: final_psbt.clone(),
        descriptors: vec![
            SecretString(seller_keys.private.btc_descriptor_xprv.clone()),
            SecretString(seller_keys.private.btc_change_descriptor_xprv.clone()),
            SecretString(seller_keys.private.rgb_assets_descriptor_xprv.clone()),
        ],
    };
    let seller_psbt_resp = sign_and_publish_psbt_file(request).await;
    assert!(seller_psbt_resp.is_ok());

    // 12. Mine Some Blocks
    let whatever_address = "bcrt1p76gtucrxhmn8s5622r859dpnmkj0kgfcel9xy0sz6yj84x6ppz2qk5hpsw";
    send_some_coins(whatever_address, "0.001").await;

    // 13. Accept Consig (Buyer/Seller)
    let all_sks = [buyer_sk.clone(), seller_sk.clone()];
    for sk in all_sks {
        let resp = verify_transfers(&sk).await;
        assert!(resp.is_ok());

        let list_resp = resp?;
        if let Some(consig_status) = list_resp
            .transfers
            .into_iter()
            .find(|x| x.consig_id == consig_id)
        {
            assert!(consig_status.is_accept);
        }
    }

    // 15. Retrieve Contract (Buyer Side)
    let resp = get_contract(&buyer_sk, &contract_id).await;
    assert!(resp.is_ok());
    assert_eq!(4.0, resp?.balance_normalized);

    // 14. Retrieve Contract (Seller Side)
    let resp = get_contract(&seller_sk, &contract_id).await;
    assert!(resp.is_ok());
    assert_eq!(1., resp?.balance_normalized);

    Ok(())
}

#[tokio::test]
async fn create_hotswap_swap_for_uda() -> anyhow::Result<()> {
    // 1. Initial Setup
    let seller_keys = new_mnemonic(&SecretString("".to_string())).await?;
    let buyer_keys = new_mnemonic(&SecretString("".to_string())).await?;

    let watcher_name = "default";
    let issuer_sk = &seller_keys.private.nostr_prv;
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: seller_keys.public.watcher_xpub.clone(),
        force: true,
    };
    create_watcher(issuer_sk, create_watch_req.clone()).await?;

    let owner_sk = &buyer_keys.private.nostr_prv;
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: buyer_keys.public.watcher_xpub.clone(),
        force: true,
    };
    create_watcher(owner_sk, create_watch_req.clone()).await?;

    // 2. Setup Wallets (Seller)
    let btc_address_1 = get_new_address(
        &SecretString(seller_keys.public.btc_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let default_coins = "0.001";
    send_some_coins(&btc_address_1, default_coins).await;

    let btc_descriptor_xprv = SecretString(seller_keys.private.btc_descriptor_xprv.clone());
    let btc_change_descriptor_xprv =
        SecretString(seller_keys.private.btc_change_descriptor_xprv.clone());

    let assets_address_1 = get_new_address(
        &SecretString(seller_keys.public.rgb_assets_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let uda_address_1 = get_new_address(
        &SecretString(seller_keys.public.rgb_udas_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let btc_wallet = get_wallet(&btc_descriptor_xprv, Some(&btc_change_descriptor_xprv)).await?;
    sync_wallet(&btc_wallet).await?;

    let fund_vault = fund_vault(
        &btc_descriptor_xprv,
        &btc_change_descriptor_xprv,
        &assets_address_1,
        &uda_address_1,
        Some(1.1),
    )
    .await?;

    // 3. Send some coins (Buyer)
    let btc_address_1 = get_new_address(
        &SecretString(buyer_keys.public.btc_descriptor_xpub.clone()),
        None,
    )
    .await?;
    let asset_address_1 = get_new_address(
        &SecretString(buyer_keys.public.rgb_udas_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let default_coins = "0.1";
    send_some_coins(&btc_address_1, default_coins).await;
    send_some_coins(&asset_address_1, default_coins).await;

    // 4. Issue Contract (Seller)
    let metadata = get_uda_data();
    let issuer_resp = issuer_issue_contract_v2(
        1,
        "RGB21",
        ContractAmount::with(1, 0, 0).to_value(),
        false,
        false,
        Some(metadata),
        None,
        Some(UtxoFilter::with_outpoint(
            fund_vault.udas_output.unwrap_or_default(),
        )),
        Some(seller_keys.clone()),
    )
    .await?;
    let IssueResponse {
        contract_id,
        iface,
        contract: contract_format,
        ..
    } = issuer_resp[0].clone();

    let buyer_sk = buyer_keys.private.nostr_prv.clone();
    let buyer_import_req = ImportRequest {
        data: contract_format.armored,
        import: AssetType::RGB21,
    };
    let buyer_import_resp = import_contract(&buyer_sk, buyer_import_req).await;
    assert!(buyer_import_resp.is_ok());

    // 5. Create Seller Swap Side
    let contract_amount = 1;
    let seller_sk = seller_keys.private.nostr_prv.clone();
    let bitcoin_price: u64 = 100_000;
    let seller_asset_desc = seller_keys.public.rgb_udas_descriptor_xpub.clone();
    let expire_at = (chrono::Local::now() + chrono::Duration::minutes(5))
        .naive_utc()
        .timestamp();

    let bid_amount = "1.0";
    let seller_swap_req = RgbOfferRequest {
        contract_id: contract_id.clone(),
        iface,
        contract_amount: bid_amount.to_string(),
        bitcoin_price,
        descriptor: SecretString(seller_asset_desc),
        change_terminal: "/21/1".to_string(),
        bitcoin_changes: vec![],
        strategy: RgbSwapStrategy::HotSwap,
        expire_at: Some(expire_at),
    };

    let seller_swap_resp = create_seller_offer(&seller_sk, seller_swap_req).await;
    assert!(seller_swap_resp.is_ok());

    // 7. Create Buyer Swap Side
    let RgbOfferResponse { offer_id, .. } = seller_swap_resp?;
    let buyer_btc_desc = buyer_keys.public.btc_descriptor_xpub.clone();
    let buyer_swap_req = RgbBidRequest {
        offer_id: offer_id.clone(),
        asset_amount: contract_amount.to_string(),
        descriptor: SecretString(buyer_btc_desc),
        change_terminal: "/1/0".to_string(),
        fee: PsbtFeeRequest::Value(1000),
    };

    let buyer_swap_resp = create_buyer_bid(&buyer_sk, buyer_swap_req).await;
    assert!(buyer_swap_resp.is_ok());

    // 8. Sign the Buyer Side
    let RgbBidResponse {
        bid_id, swap_psbt, ..
    } = buyer_swap_resp?;

    let request = SignPsbtRequest {
        psbt: swap_psbt,
        descriptors: vec![
            SecretString(buyer_keys.private.btc_descriptor_xprv.clone()),
            SecretString(buyer_keys.private.btc_change_descriptor_xprv.clone()),
        ],
    };
    let buyer_psbt_resp = sign_psbt_file(request).await;
    assert!(buyer_psbt_resp.is_ok());

    // 9. Create Swap PSBT
    let SignedPsbtResponse {
        psbt: swap_psbt, ..
    } = buyer_psbt_resp?;
    let final_swap_req = RgbSwapRequest {
        offer_id,
        bid_id,
        swap_psbt: swap_psbt.clone(),
    };

    let final_swap_resp = create_swap_transfer(issuer_sk, final_swap_req).await;
    assert!(final_swap_resp.is_ok());

    // 8. Sign the Final PSBT
    let RgbSwapResponse {
        final_consig,
        final_psbt,
        ..
    } = final_swap_resp?;

    let request = SignPsbtRequest {
        psbt: final_psbt.clone(),
        descriptors: vec![
            SecretString(seller_keys.private.btc_descriptor_xprv.clone()),
            SecretString(seller_keys.private.btc_change_descriptor_xprv.clone()),
            SecretString(seller_keys.private.rgb_udas_descriptor_xprv.clone()),
        ],
    };
    let seller_psbt_resp = sign_and_publish_psbt_file(request).await;
    assert!(seller_psbt_resp.is_ok());

    // 9. Accept Consig (Buyer/Seller)
    let all_sks = [buyer_sk.clone(), seller_sk.clone()];
    for sk in all_sks {
        let request = AcceptRequest {
            consignment: final_consig.clone(),
            force: false,
        };
        let resp = accept_transfer(&sk, request).await;
        assert!(resp.is_ok());
        assert!(resp?.valid);
    }

    // 10 Mine Some Blocks
    let whatever_address = "bcrt1p76gtucrxhmn8s5622r859dpnmkj0kgfcel9xy0sz6yj84x6ppz2qk5hpsw";
    send_some_coins(whatever_address, "0.001").await;

    // 11. Retrieve Contract (Seller Side)
    let resp = get_contract(&seller_sk, &contract_id).await;
    assert!(resp.is_ok());
    assert_eq!(0., resp?.balance_normalized);

    // 12. Retrieve Contract (Buyer Side)
    let resp = get_contract(&buyer_sk, &contract_id).await;
    assert!(resp.is_ok());
    assert_eq!(1., resp?.balance_normalized);

    // 13. Verify transfers (Seller Side)
    let resp = verify_transfers(&seller_sk).await;
    assert!(resp.is_ok());
    assert_eq!(1, resp?.transfers.len());

    Ok(())
}

#[tokio::test]
async fn create_p2p_swap() -> anyhow::Result<()> {
    // 1. Initial Setup
    let seller_keys = new_mnemonic(&SecretString("".to_string())).await?;
    let buyer_keys = new_mnemonic(&SecretString("".to_string())).await?;

    let watcher_name = "default";
    let seller_sk = seller_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: seller_keys.public.watcher_xpub.clone(),
        force: true,
    };
    create_watcher(&seller_sk, create_watch_req.clone()).await?;

    let buyer_sk = buyer_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: buyer_keys.public.watcher_xpub.clone(),
        force: true,
    };
    create_watcher(&buyer_sk, create_watch_req.clone()).await?;

    // 2. Setup Wallets (Seller)
    let btc_address_1 = get_new_address(
        &SecretString(seller_keys.public.btc_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let default_coins = "0.001";
    send_some_coins(&btc_address_1, default_coins).await;

    let btc_descriptor_xprv = SecretString(seller_keys.private.btc_descriptor_xprv.clone());
    let btc_change_descriptor_xprv =
        SecretString(seller_keys.private.btc_change_descriptor_xprv.clone());

    let assets_address_1 = get_new_address(
        &SecretString(seller_keys.public.rgb_assets_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let uda_address_1 = get_new_address(
        &SecretString(seller_keys.public.rgb_udas_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let btc_wallet = get_wallet(&btc_descriptor_xprv, Some(&btc_change_descriptor_xprv)).await?;
    sync_wallet(&btc_wallet).await?;

    let fund_vault = fund_vault(
        &btc_descriptor_xprv,
        &btc_change_descriptor_xprv,
        &assets_address_1,
        &uda_address_1,
        Some(1.1),
    )
    .await?;

    // 3. Send some coins (Buyer)
    let btc_address_1 = get_new_address(
        &SecretString(buyer_keys.public.btc_descriptor_xpub.clone()),
        None,
    )
    .await?;
    let asset_address_1 = get_new_address(
        &SecretString(buyer_keys.public.rgb_assets_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let default_coins = "0.1";
    send_some_coins(&btc_address_1, default_coins).await;
    send_some_coins(&asset_address_1, default_coins).await;

    // 4. Issue Contract (Seller)
    let issuer_resp = issuer_issue_contract_v2(
        1,
        "RGB20",
        ContractAmount::with(5, 0, 2).to_value(),
        false,
        false,
        None,
        None,
        Some(UtxoFilter::with_outpoint(
            fund_vault.assets_output.unwrap_or_default(),
        )),
        Some(seller_keys.clone()),
    )
    .await?;

    let IssueResponse {
        contract_id,
        iface,
        supply,
        contract,
        precision,
        ..
    } = issuer_resp[0].clone();

    let buyer_import_req = ImportRequest {
        import: AssetType::RGB20,
        data: contract.strict,
    };
    let buyer_import_resp = import_contract(&buyer_sk, buyer_import_req).await;
    assert!(buyer_import_resp.is_ok());

    // 5. Create Seller Swap Side
    let contract_amount = supply - 1;
    let bitcoin_price: u64 = 100_001;
    let seller_asset_desc = seller_keys.public.rgb_assets_descriptor_xpub.clone();
    let expire_at = (chrono::Local::now() + chrono::Duration::minutes(5))
        .naive_utc()
        .timestamp();

    let asset_amount = ContractAmount::new(contract_amount, precision).to_string();
    let seller_swap_req = RgbOfferRequest {
        contract_id: contract_id.clone(),
        iface: iface.clone(),
        contract_amount: asset_amount,
        bitcoin_price,
        descriptor: SecretString(seller_asset_desc),
        change_terminal: "/20/1".to_string(),
        bitcoin_changes: vec![],
        expire_at: Some(expire_at),
        strategy: RgbSwapStrategy::P2P,
    };

    let seller_swap_resp = create_seller_offer(&seller_sk, seller_swap_req).await;
    assert!(seller_swap_resp.is_ok());

    // 6. Sign the Seller PSBT
    let RgbOfferResponse {
        offer_id,
        seller_psbt,
        ..
    } = seller_swap_resp?;

    let seller_psbt_req = SignPsbtRequest {
        psbt: seller_psbt.clone(),
        descriptors: vec![
            SecretString(seller_keys.private.btc_descriptor_xprv.clone()),
            SecretString(seller_keys.private.btc_change_descriptor_xprv.clone()),
            SecretString(seller_keys.private.rgb_assets_descriptor_xprv.clone()),
        ],
    };
    let seller_psbt_resp = sign_psbt_file(seller_psbt_req).await;
    assert!(seller_psbt_resp.is_ok());

    let SignedPsbtResponse { psbt, .. } = seller_psbt_resp?;
    let update_offer_req = RgbOfferUpdateRequest {
        contract_id: contract_id.clone(),
        offer_id: offer_id.clone(),
        offer_psbt: psbt.clone(),
    };
    let update_offer_resp = update_seller_offer(&seller_sk, update_offer_req).await;
    assert!(update_offer_resp.is_ok());

    // 7. Create Buyer Swap Side
    let bid_amount = "4.0";
    let buyer_btc_desc = buyer_keys.public.btc_descriptor_xpub.clone();
    let buyer_swap_req = RgbBidRequest {
        offer_id: offer_id.clone(),
        asset_amount: bid_amount.to_string(),
        descriptor: SecretString(buyer_btc_desc),
        change_terminal: "/1/0".to_string(),
        fee: PsbtFeeRequest::Value(1000),
    };

    let buyer_swap_resp = create_buyer_bid(&buyer_sk, buyer_swap_req).await;
    assert!(buyer_swap_resp.is_ok());

    // 9. Create Swap PSBT
    let RgbBidResponse {
        bid_id, swap_psbt, ..
    } = buyer_swap_resp?;
    let final_swap_req = RgbSwapRequest {
        offer_id,
        bid_id,
        swap_psbt: swap_psbt.clone(),
    };

    let final_swap_resp = create_swap_transfer(&buyer_sk, final_swap_req).await;
    assert!(final_swap_resp.is_ok());

    let RgbSwapResponse {
        final_psbt,
        consig_id,
        ..
    } = final_swap_resp?;

    // 8. Sign the Buyer Side
    let buyer_psbt_req = SignPsbtRequest {
        psbt: final_psbt,
        descriptors: vec![
            SecretString(buyer_keys.private.btc_descriptor_xprv.clone()),
            SecretString(buyer_keys.private.btc_change_descriptor_xprv.clone()),
        ],
    };
    let buyer_psbt_resp = sign_psbt_file(buyer_psbt_req).await;
    assert!(buyer_psbt_resp.is_ok());
    let SignedPsbtResponse { psbt, .. } = buyer_psbt_resp?;

    // 10. Publish Swap PSBT
    let final_swap_req = PublishPsbtRequest { psbt };
    let published_psbt_resp = publish_psbt_file(final_swap_req).await;
    assert!(published_psbt_resp.is_ok());

    // 11. Mine Some Blocks
    let whatever_address = "bcrt1p76gtucrxhmn8s5622r859dpnmkj0kgfcel9xy0sz6yj84x6ppz2qk5hpsw";
    send_some_coins(whatever_address, "0.001").await;

    // 12. Accept Consig (Buyer/Seller)
    let all_sks = [buyer_sk.clone(), seller_sk.clone()];
    for sk in all_sks {
        let resp = verify_transfers(&sk).await;
        assert!(resp.is_ok());

        let list_resp = resp?;
        if let Some(consig_status) = list_resp
            .transfers
            .into_iter()
            .find(|x| x.consig_id == consig_id)
        {
            assert!(consig_status.is_accept);
        }
    }

    // 14. Retrieve Contract (Buyer Side)
    let resp = get_contract(&buyer_sk, &contract_id).await;
    assert!(resp.is_ok());
    assert_eq!(4., resp?.balance_normalized);

    // 13. Retrieve Contract (Seller Side)
    let resp = get_contract(&seller_sk, &contract_id).await;
    assert!(resp.is_ok());
    assert_eq!(1., resp?.balance_normalized);

    Ok(())
}

#[tokio::test]
async fn create_auction_swap() -> anyhow::Result<()> {
    init_logging("bitmask_core=debug");

    // 1. Initial Setup
    let seller_keys = new_mnemonic(&SecretString("".to_string())).await?;
    let buyer_keys = new_mnemonic(&SecretString("".to_string())).await?;

    let watcher_name = "default";
    let seller_sk = seller_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: seller_keys.public.watcher_xpub.clone(),
        force: true,
    };
    create_watcher(&seller_sk, create_watch_req.clone()).await?;

    let buyer_sk = buyer_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: buyer_keys.public.watcher_xpub.clone(),
        force: true,
    };
    create_watcher(&buyer_sk, create_watch_req.clone()).await?;

    // 2. Setup Wallets (Seller)
    let btc_address_1 = get_new_address(
        &SecretString(seller_keys.public.btc_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let default_coins = "0.001";
    send_some_coins(&btc_address_1, default_coins).await;

    let btc_descriptor_xprv = SecretString(seller_keys.private.btc_descriptor_xprv.clone());
    let btc_change_descriptor_xprv =
        SecretString(seller_keys.private.btc_change_descriptor_xprv.clone());

    let assets_address_1 = get_new_address(
        &SecretString(seller_keys.public.rgb_assets_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let uda_address_1 = get_new_address(
        &SecretString(seller_keys.public.rgb_udas_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let btc_wallet = get_wallet(&btc_descriptor_xprv, Some(&btc_change_descriptor_xprv)).await?;
    sync_wallet(&btc_wallet).await?;

    let fund_vault = fund_vault(
        &btc_descriptor_xprv,
        &btc_change_descriptor_xprv,
        &assets_address_1,
        &uda_address_1,
        Some(1.1),
    )
    .await?;

    // 3. Send some coins (Buyer)
    let btc_address_1 = get_new_address(
        &SecretString(buyer_keys.public.btc_descriptor_xpub.clone()),
        None,
    )
    .await?;
    let asset_address_1 = get_new_address(
        &SecretString(buyer_keys.public.rgb_assets_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let default_coins = "0.1";
    send_some_coins(&btc_address_1, default_coins).await;
    send_some_coins(&asset_address_1, default_coins).await;

    // 4. Issue Contract (Seller)
    let issuer_resp = issuer_issue_contract_v2(
        3,
        "RGB20",
        ContractAmount::with(2, 0, 2).to_value(),
        false,
        false,
        None,
        None,
        Some(UtxoFilter::with_outpoint(
            fund_vault.assets_output.unwrap_or_default(),
        )),
        Some(seller_keys.clone()),
    )
    .await?;

    for contract in issuer_resp.clone() {
        let buyer_import_req = ImportRequest {
            import: AssetType::RGB20,
            data: contract.contract.strict,
        };
        let buyer_import_resp = import_contract(&buyer_sk, buyer_import_req).await;
        assert!(buyer_import_resp.is_ok());
    }

    // 5. Create Collection (Seller)
    let contract_amount = "1.00".to_string();
    let mut offers_collection = vec![];
    for contract in issuer_resp.clone() {
        let IssueResponse {
            contract_id, iface, ..
        } = contract.clone();

        let desc = SecretString(seller_keys.public.rgb_assets_descriptor_xpub.clone());
        let req = RgbOfferRequest {
            contract_id,
            iface,
            contract_amount: contract_amount.clone(),
            bitcoin_price: 1_000,
            descriptor: desc,
            change_terminal: "/20/1".to_string(),
            bitcoin_changes: vec![],
            strategy: RgbSwapStrategy::Auction,
            expire_at: None,
        };

        offers_collection.push(req);
    }

    let offer_auction_req = RgbAuctionOfferRequest {
        offers: offers_collection.clone(),
        sign_keys: vec![
            SecretString(seller_keys.private.btc_descriptor_xprv.clone()),
            SecretString(seller_keys.private.btc_change_descriptor_xprv.clone()),
            SecretString(seller_keys.private.rgb_assets_descriptor_xprv.clone()),
        ],
    };

    let resp = create_auction_offers(&seller_sk, offer_auction_req).await;
    assert!(resp.is_ok());

    let mut offers = resp?;
    let RgbOfferResponse {
        offer_id: offer_1st,
        contract_id: contract_1st,
        bundle_id,
        ..
    } = offers.remove(0);

    let RgbOfferResponse {
        offer_id: offer_2nd,
        contract_id: contract_2nd,
        ..
    } = offers.remove(0);

    // 6. Create Bid (1st Offer)
    let buyer_btc_desc = buyer_keys.public.btc_descriptor_xpub.clone();
    let bid_auction_req = RgbAuctionBidRequest {
        offer_id: offer_1st.clone(),
        asset_amount: contract_amount.clone(),
        descriptor: SecretString(buyer_btc_desc),
        change_terminal: "/1/0".to_string(),
        fee: PsbtFeeRequest::Value(1000),
        sign_keys: vec![
            SecretString(buyer_keys.private.btc_descriptor_xprv.clone()),
            SecretString(buyer_keys.private.btc_change_descriptor_xprv.clone()),
        ],
    };

    let resp = create_auction_bid(&buyer_sk, bid_auction_req).await;
    assert!(resp.is_ok());

    // 7. Create Bid (2nd Offer)
    let buyer_btc_desc = buyer_keys.public.btc_descriptor_xpub.clone();
    let bid_auction_req = RgbAuctionBidRequest {
        offer_id: offer_2nd.clone(),
        asset_amount: contract_amount.clone(),
        descriptor: SecretString(buyer_btc_desc),
        change_terminal: "/1/0".to_string(),
        fee: PsbtFeeRequest::Value(1000),
        sign_keys: vec![
            SecretString(buyer_keys.private.btc_descriptor_xprv.clone()),
            SecretString(buyer_keys.private.btc_change_descriptor_xprv.clone()),
        ],
    };

    let resp = create_auction_bid(&buyer_sk, bid_auction_req).await;
    assert!(resp.is_ok());

    // 7. Finish Offer
    let resp = finish_auction_offers(&seller_sk, bundle_id.unwrap_or_default()).await;
    assert!(resp.is_ok());

    // 8. Mine Some Blocks
    generate_new_block().await;

    // 10. Verify Transfers
    let all_sks = [seller_sk.clone(), buyer_sk.clone()];
    for sk in all_sks {
        let resp = verify_transfers(&sk).await;
        assert!(resp.is_ok());
    }

    // 11. Check Balances (1st Offer)
    let resp = get_contract(&buyer_sk, &contract_1st).await;
    assert!(resp.is_ok());
    assert_eq!(1., resp?.balance_normalized);

    let resp = get_contract(&seller_sk, &contract_1st).await;
    assert!(resp.is_ok());
    assert_eq!(1., resp?.balance_normalized);

    // // 12. Check Balances (2nd Offer)
    let resp = get_contract(&buyer_sk, &contract_2nd).await;
    assert!(resp.is_ok());
    // println!("{:#?}", resp?.allocations);
    assert_eq!(1., resp?.balance_normalized);

    let resp = get_contract(&seller_sk, &contract_2nd).await;
    assert!(resp.is_ok());
    // println!("{:#?}", resp?.allocations);
    assert_eq!(1., resp?.balance_normalized);

    Ok(())
}

#[tokio::test]
async fn create_collectible_auction() -> anyhow::Result<()> {
    init_logging("bitmask_core=debug");

    // 1. Initial Setup
    let alice_keys = new_mnemonic(&SecretString("".to_string())).await?;
    let bob_keys = new_mnemonic(&SecretString("".to_string())).await?;

    let watcher_name = "default";
    let alice_sk = alice_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: alice_keys.public.watcher_xpub.clone(),
        force: true,
    };
    create_watcher(&alice_sk, create_watch_req.clone()).await?;

    let bob_sk = bob_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: bob_keys.public.watcher_xpub.clone(),
        force: true,
    };
    create_watcher(&bob_sk, create_watch_req.clone()).await?;

    // 2. Setup Wallets (Seller)
    let btc_address_1 = get_new_address(
        &SecretString(alice_keys.public.btc_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let default_coins = "0.001";
    send_some_coins(&btc_address_1, default_coins).await;

    let btc_descriptor_xprv = SecretString(alice_keys.private.btc_descriptor_xprv.clone());
    let btc_change_descriptor_xprv =
        SecretString(alice_keys.private.btc_change_descriptor_xprv.clone());

    let assets_address_1 = get_new_address(
        &SecretString(alice_keys.public.rgb_assets_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let uda_address_1 = get_new_address(
        &SecretString(alice_keys.public.rgb_udas_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let btc_wallet = get_wallet(&btc_descriptor_xprv, Some(&btc_change_descriptor_xprv)).await?;
    sync_wallet(&btc_wallet).await?;

    let fund_vault = fund_vault(
        &btc_descriptor_xprv,
        &btc_change_descriptor_xprv,
        &assets_address_1,
        &uda_address_1,
        Some(1.1),
    )
    .await?;

    // 3. Send some coins (Buyer)
    let btc_address_1 = get_new_address(
        &SecretString(bob_keys.public.btc_descriptor_xpub.clone()),
        None,
    )
    .await?;
    let asset_address_1 = get_new_address(
        &SecretString(bob_keys.public.rgb_udas_descriptor_xpub.clone()),
        None,
    )
    .await?;

    let default_coins = "0.1";
    send_some_coins(&btc_address_1, default_coins).await;
    send_some_coins(&asset_address_1, default_coins).await;

    // 4. Issue Contract (Seller)
    let metadata = get_uda_data();
    let issuer_resp = issuer_issue_contract_v2(
        2,
        "RGB21",
        ContractAmount::with(1, 0, 0).to_value(),
        false,
        false,
        Some(metadata),
        None,
        Some(UtxoFilter::with_outpoint(
            fund_vault.udas_output.unwrap_or_default(),
        )),
        Some(alice_keys.clone()),
    )
    .await?;

    for contract in issuer_resp.clone() {
        let bob_import_req = ImportRequest {
            import: AssetType::RGB20,
            data: contract.contract.strict,
        };
        let bob_import_resp = import_contract(&bob_sk, bob_import_req).await;
        assert!(bob_import_resp.is_ok());
    }

    // 5. Create Collection (Seller)
    let contract_amount = "1.00".to_string();
    let mut offers_collection = vec![];
    for contract in issuer_resp.clone() {
        let IssueResponse {
            contract_id, iface, ..
        } = contract.clone();

        let desc = SecretString(alice_keys.public.rgb_udas_descriptor_xpub.clone());
        let req = RgbOfferRequest {
            contract_id,
            iface,
            contract_amount: contract_amount.clone(),
            bitcoin_price: 1_000,
            descriptor: desc,
            change_terminal: "/21/1".to_string(),
            bitcoin_changes: vec![],
            strategy: RgbSwapStrategy::Auction,
            expire_at: None,
        };

        offers_collection.push(req);
    }

    let offer_auction_req = RgbAuctionOfferRequest {
        offers: offers_collection.clone(),
        sign_keys: vec![
            SecretString(alice_keys.private.btc_descriptor_xprv.clone()),
            SecretString(alice_keys.private.btc_change_descriptor_xprv.clone()),
            SecretString(alice_keys.private.rgb_udas_descriptor_xprv.clone()),
        ],
    };

    let resp = create_auction_offers(&alice_sk, offer_auction_req).await;
    assert!(resp.is_ok());

    let mut offers = resp?;
    let RgbOfferResponse {
        offer_id: offer_1st,
        contract_id: contract_1st,
        bundle_id,
        ..
    } = offers.remove(0);

    let RgbOfferResponse {
        offer_id: offer_2nd,
        contract_id: contract_2nd,
        ..
    } = offers.remove(0);

    // 6. Create Bid (1st Offer)
    let bob_btc_desc = bob_keys.public.btc_descriptor_xpub.clone();
    let bid_auction_req = RgbAuctionBidRequest {
        offer_id: offer_1st.clone(),
        asset_amount: contract_amount.clone(),
        descriptor: SecretString(bob_btc_desc),
        change_terminal: "/1/0".to_string(),
        fee: PsbtFeeRequest::Value(1000),
        sign_keys: vec![
            SecretString(bob_keys.private.btc_descriptor_xprv.clone()),
            SecretString(bob_keys.private.btc_change_descriptor_xprv.clone()),
        ],
    };

    let resp = create_auction_bid(&bob_sk, bid_auction_req).await;
    assert!(resp.is_ok());

    // 7. Create Bid (2nd Offer)
    let bob_btc_desc = bob_keys.public.btc_descriptor_xpub.clone();
    let bid_auction_req = RgbAuctionBidRequest {
        offer_id: offer_2nd.clone(),
        asset_amount: contract_amount.clone(),
        descriptor: SecretString(bob_btc_desc),
        change_terminal: "/1/0".to_string(),
        fee: PsbtFeeRequest::Value(1000),
        sign_keys: vec![
            SecretString(bob_keys.private.btc_descriptor_xprv.clone()),
            SecretString(bob_keys.private.btc_change_descriptor_xprv.clone()),
        ],
    };

    let resp = create_auction_bid(&bob_sk, bid_auction_req).await;
    assert!(resp.is_ok());

    // 7. Finish Offer
    let resp = finish_auction_offers(&alice_sk, bundle_id.unwrap_or_default()).await;
    assert!(resp.is_ok());

    // 8. Mine Some Blocks
    generate_new_block().await;

    // 10. Verify Transfers
    let all_sks = [bob_sk.clone(), alice_sk.clone()];
    for sk in all_sks {
        let resp = verify_transfers(&sk).await;
        assert!(resp.is_ok());
    }

    // 11. Check Balances (1st Offer)
    let resp = get_contract(&bob_sk, &contract_1st).await;
    assert!(resp.is_ok());
    assert_eq!(1., resp?.balance_normalized);

    let resp = get_contract(&alice_sk, &contract_1st).await;
    assert!(resp.is_ok());
    assert_eq!(0., resp?.balance_normalized);

    // // 12. Check Balances (2nd Offer)
    let resp = get_contract(&bob_sk, &contract_2nd).await;
    assert!(resp.is_ok());
    assert_eq!(1., resp?.balance_normalized);

    let resp = get_contract(&alice_sk, &contract_2nd).await;
    assert!(resp.is_ok());
    assert_eq!(0., resp?.balance_normalized);

    Ok(())
}
