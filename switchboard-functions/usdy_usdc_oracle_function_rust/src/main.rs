use crate::solana_sdk::commitment_config::CommitmentConfig;
use ethers::providers::{Http, Provider};
use ethers::types::*;
use ethers_contract_derive::abigen;
use futures::future::join_all;
use rust_decimal::Decimal;
use std::boxed::Box;
use std::cmp::Ordering;
use std::pin::Pin;
use std::str::FromStr;
use switchboard_solana::prelude::*;
use switchboard_utils;
use switchboard_utils::FromPrimitive;
use tokio;
use std::sync::Arc;

abigen!(Factory, "./abis/factory.json");
abigen!(Pool, "./abis/pool.json");
abigen!(Ondo, "./abis/ondo.json");

fn median(mut values: Vec<Decimal>) -> Decimal {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / Decimal::TWO
    } else {
        values[mid]
    }
}

fn to_u8_array(input: &str) -> [u8; 32] {
    let mut array = [0u8; 32];
    let bytes = input.as_bytes();
    let length = bytes.len().min(32); // Ensure that we don't exceed the array length
    array[..length].copy_from_slice(&bytes[..length]);
    array
}

async fn uniswap_quote(
    ether_transport: Provider<Http>,
    factory_addr: H160,
    token1: H160,
    token2: H160,
) -> Result<Decimal, Error> {
    let factory_contract = Factory::new(factory_addr, ether_transport.clone().into());

    let pool = factory_contract
        .get_pool(token1, token2, 500)
        .call()
        .await
        .map_err(|_| Error::FetchError)?;

    let pool_contract = Pool::new(pool, ether_transport.into());
    let slot0 = pool_contract
        .slot_0()
        .call()
        .await
        .map_err(|_| Error::FetchError)?;
    //    sqrtPriceX96 = sqrt(price) * 2 ** 96

    let sqrt_price_x96: U256 = slot0.0;
    let price: U256 = (sqrt_price_x96 * sqrt_price_x96) >> (96 * 2);

    let inverse_price: f64 = 0.000001 / (price.as_u128() as f64);
    let inverse_price = 1.0 / inverse_price;
    let inverse_price = 1_000_000_000_000_000_000.0 / inverse_price * 1_000_000_000_000_000_000.0;
    Ok(Decimal::from_f64(inverse_price).unwrap())
}

async fn get_ondo_price(ether_transport: Provider<Http>) -> Result<Decimal, Error> {
    let ondo = Ondo::new(
        H160::from_str("0xa0219aa5b31e65bc920b5b6dfb8edf0988121de0").unwrap(),
        ether_transport.clone().into(),
    );

    let price = ondo
        .get_price()
        .call()
        .await
        .map_err(|_| Error::FetchError)?;
    let price: u128 = price.as_u128();
    Ok(Decimal::from_u128(price).unwrap())
}

pub async fn fetch_all<T, E>(v: Vec<Pin<Box<dyn Future<Output = Result<T, E>> + Send>>>) -> Result<Vec<T>, E> {
    join_all(v).await.into_iter().collect()
}

#[switchboard_function]
pub async fn sb_function(
    runner: Arc<FunctionRunner>,
    _: Vec<u8>,
) -> Result<Vec<Instruction>, SbFunctionError> {
    runner.set_priority_fee(1000).await;
    let mantle_tp = Provider::try_from("https://mantle.publicnode.com").unwrap();
    let ether_tp = Provider::try_from("https://ethereum.publicnode.com").unwrap();
    let agni_factory = H160::from_str("0x25780dc8Fc3cfBD75F33bFDAB65e969b603b2035").unwrap();

    let fusion_factory = H160::from_str("0x530d2766D1988CC1c000C8b7d00334c14B69AD71").unwrap();
    let usdy = H160::from_str("0x5bE26527e817998A7206475496fDE1E68957c5A6").unwrap();
    let usd = H160::from_str("0x09Bc4E0D864854c6aFB6eB9A9cdF58aC190D0dF9").unwrap();

    let scale = Decimal::from(10u64.pow(18));

    let usdy_decimals: Vec<_> = fetch_all(vec![
        Box::pin(uniswap_quote(mantle_tp.clone(), agni_factory, usdy, usd)),
        Box::pin(uniswap_quote(mantle_tp.clone(), fusion_factory, usdy, usd)),
        Box::pin(get_ondo_price(ether_tp)),
    ]).await?.into_iter().map(|x| x / scale).collect();

    let mkt_median = median(usdy_decimals[0..2].to_vec());
    let ondo_price = usdy_decimals[2];

    // these will not be correct without a specified function key
    let market_price_ix = runner.upsert_feed(&to_u8_array("USDY_MEDIAN"), mkt_median);
    let ondo_price_ix = runner.upsert_feed(&to_u8_array("ONDO_PRICE"), ondo_price);
    println!("USDY Median:{}, address:{}", mkt_median, market_price_ix.0);
    println!("Ondo price:{}, address:{}", ondo_price, ondo_price_ix.0);
    Ok(vec![market_price_ix.1, ondo_price_ix.1])
}

#[sb_error]
pub enum Error {
    InvalidResult,
    FetchError,
    ConversionError,
}
