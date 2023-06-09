use std::{collections::BTreeMap, sync::Arc};

use anyhow::Result;
use bdk::{blockchain::esplora::EsploraBlockchain, database::MemoryDatabase, SyncOptions, Wallet};
use once_cell::sync::Lazy;
use tokio::sync::{Mutex, RwLock};

use crate::{
    constants::{BITCOIN_EXPLORER_API, NETWORK},
    debug,
};

pub type MemoryWallet = Arc<Mutex<Wallet<MemoryDatabase>>>;
type Wallets = BTreeMap<(String, Option<String>), MemoryWallet>;

#[derive(Default)]
struct Networks {
    bitcoin: Arc<RwLock<Wallets>>,
    testnet: Arc<RwLock<Wallets>>,
    signet: Arc<RwLock<Wallets>>,
    regtest: Arc<RwLock<Wallets>>,
}

static BDK: Lazy<Networks> = Lazy::new(Networks::default);

pub async fn get_wallet(
    descriptor: &str,
    change_descriptor: Option<String>,
) -> Result<Arc<Mutex<Wallet<MemoryDatabase>>>> {
    let network = NETWORK.read().await.to_owned();
    let descriptor = descriptor.to_owned();
    let key = (descriptor.clone(), change_descriptor.clone());

    let wallets = match network {
        bitcoin::Network::Bitcoin => BDK.bitcoin.clone(),
        bitcoin::Network::Testnet => BDK.testnet.clone(),
        bitcoin::Network::Signet => BDK.signet.clone(),
        bitcoin::Network::Regtest => BDK.regtest.clone(),
    };

    match wallets.clone().read().await.get(&key) {
        Some(wallet) => Ok(wallet.clone()),
        None => {
            let new_wallet = Arc::new(Mutex::new(Wallet::new(
                &descriptor,
                change_descriptor.as_ref(),
                network,
                MemoryDatabase::default(),
            )?));

            match network {
                bitcoin::Network::Bitcoin => {
                    BDK.bitcoin.write().await.insert(key, new_wallet.clone());
                }
                bitcoin::Network::Testnet => {
                    BDK.testnet.write().await.insert(key, new_wallet.clone());
                }
                bitcoin::Network::Signet => {
                    BDK.signet.write().await.insert(key, new_wallet.clone());
                }
                bitcoin::Network::Regtest => {
                    BDK.regtest.write().await.insert(key, new_wallet.clone());
                }
            };
            panic!("get_wallet");

            Ok(new_wallet)
        }
    }
}

pub async fn get_blockchain() -> EsploraBlockchain {
    debug!("Getting blockchain");
    EsploraBlockchain::new(&BITCOIN_EXPLORER_API.read().await, 100)
}

pub async fn synchronize_wallet(wallet: &MemoryWallet) -> Result<()> {
    let blockchain = get_blockchain().await;
    wallet
        .lock()
        .await
        .sync(&blockchain, SyncOptions::default())
        .await?;
    debug!("Synced");
    Ok(())
}
