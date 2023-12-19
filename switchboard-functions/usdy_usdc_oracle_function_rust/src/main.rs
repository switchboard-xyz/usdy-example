pub use switchboard_solana::prelude::*;
use crate::solana_sdk::commitment_config::CommitmentConfig;
use ethers::providers::{Http, Provider};
use ethers::types::*;
use ethers_contract_derive::abigen;
use futures::future::join_all;
use rust_decimal::Decimal;
use std::boxed::Box;
use std::pin::Pin;
use std::str::FromStr;
use switchboard_utils::FromPrimitive;

use switchboard_utils;
use tokio;
use std::cmp::Ordering;

abigen!(Factory, "./abis/factory.json");
abigen!(Pool, "./abis/pool.json");
abigen!(Ondo, "./abis/ondo.json");

fn median(mut values: Vec<Decimal>) -> Decimal {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / Decimal::from(2u64)
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

async fn get_uniswap_price(
    ether_transport: Provider<Http>,
    factory_addr: H160,
    usd_h160: H160,
    usdy_h160: H160,
) -> Result<Decimal, Error> {
    let factory_contract = Factory::new(factory_addr, ether_transport.clone().into());

    let pool = factory_contract
        .get_pool(usd_h160, usdy_h160, 500)
        .call()
        .await
        .map_err(|_| Error::FetchError)?;

    let pool_contract = Pool::new(pool, ether_transport.into());
    let slot0 = pool_contract.slot_0().call().await.map_err(|_| Error::FetchError)?;
    //    sqrtPriceX96 = sqrt(price) * 2 ** 96

    let sqrt_price_x96: U256 = slot0.0;
    let price: U256 = (sqrt_price_x96 * sqrt_price_x96) >> (96 * 2);

    let inverse_price: f64 = 0.000001 / (price.as_u128() as f64);
    let inverse_price = 1.0 / inverse_price;
    let inverse_price = 1_000_000_000_000_000_000.0 / inverse_price * 1_000_000_000_000_000_000.0;
    println!("Uniswap price: {:?}", &inverse_price);
    Ok(Decimal::from_f64(inverse_price).unwrap())
}

async fn get_ondo_price(ether_transport: Provider<Http>) -> Result<Decimal, Error> {
    let ondo = Ondo::new(
        H160::from_str("0xa0219aa5b31e65bc920b5b6dfb8edf0988121de0").unwrap(),
        ether_transport.clone().into(),
    );

    let price = ondo.get_price().call().await.map_err(|_| Error::FetchError)?;
    let price: u128 = price.as_u128();
    Ok(Decimal::from_u128(price).unwrap())
}

#[switchboard_function]
pub async fn sb_function(
    runner: FunctionRunner,
    _: Vec<u8>,
) -> Result<Vec<Instruction>, SbFunctionError> {
    let mantle_transport = Provider::try_from("https://mantle.publicnode.com").unwrap();
    let ether_transport = Provider::try_from("https://ethereum.publicnode.com").unwrap();
    let agni_factory = H160::from_str("0x25780dc8Fc3cfBD75F33bFDAB65e969b603b2035").unwrap();

    let fusion_factory = H160::from_str("0x530d2766D1988CC1c000C8b7d00334c14B69AD71").unwrap();
    let usdy_h160 = H160::from_str("0x5bE26527e817998A7206475496fDE1E68957c5A6").unwrap();
    let usd_h160 = H160::from_str("0x09Bc4E0D864854c6aFB6eB9A9cdF58aC190D0dF9").unwrap();

    let v: Vec<Pin<Box<dyn Future<Output = Result<Decimal, Error>> + Send>>> = vec![
        Box::pin(get_uniswap_price(
            mantle_transport.clone(),
            agni_factory,
            usd_h160,
            usdy_h160,
        )),
        Box::pin(get_uniswap_price(
            mantle_transport.clone(),
            fusion_factory,
            usd_h160,
            usdy_h160,
        )),
        Box::pin(get_ondo_price(ether_transport.clone())),
    ];
    let usdy_decimals: Vec<Decimal> = join_all(v).await.into_iter().map(|x| x.unwrap()).collect();
    let ondo_price = usdy_decimals.last().unwrap().clone() / Decimal::from(10u64.pow(18));
    println!("Ondo price: {:?}", ondo_price);

    let uniswap_jobs = usdy_decimals[0..2].to_vec();
    let usdy_decimals = uniswap_jobs
        .into_iter()
        .map(|x| x / Decimal::from(10u64.pow(18)))
        .collect::<Vec<Decimal>>();
    let usdy_median = median(usdy_decimals.clone());

    // these will not be correct without a specified function key
    let market_price_ix = runner.upsert_feed(&to_u8_array("USDY_MEDIAN"), usdy_median);
    let ondo_price_ix = runner.upsert_feed(&to_u8_array("ONDO_PRICE"), ondo_price.clone());
    println!("USDY Median: {}, address: {}", usdy_median, market_price_ix.0);
    println!("Ondo price: {:?}, address {}", ondo_price, ondo_price_ix.0);
    Ok(vec![market_price_ix.1, ondo_price_ix.1])
}

#[sb_error]
pub enum Error {
    InvalidResult,
    FetchError,
    ConversionError,
}
