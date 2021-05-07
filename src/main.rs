use core::fmt;
use std::{env, iter::Map, os::linux::raw::stat, thread::sleep, time::Duration};

use telegram_bot::*;

use tokio_binance::WithdrawalClient;
use serde_json::{Value};

use chrono::Utc;

const MAX_API_RETRY: i32 = 5;
const REFRESH_RATE: u64 = 60; // in seconds

#[derive(PartialEq)]
struct CoinStatus {
    network: String,
    deposit: bool,
    deposit_desc: String,
    withdraw: bool,
    withdraw_desc: String,
}

#[derive(PartialEq)]
struct CoinNetwork {
    networks: Vec<CoinStatus>
}

impl fmt::Display for CoinStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Network: {}", self.network).unwrap();

        match self.deposit {
            true => {
                writeln!(f, "Deposit available").unwrap();
            }
            false => {
                writeln!(f, "Deposit suspended: {}", self.deposit_desc).unwrap();
            }
        }
        match self.withdraw {
            true => {
                writeln!(f, "Withdraw available").unwrap();
            }
            false => {
                write!(f, "Withdraw suspended: {}", self.withdraw_desc).unwrap();
            }
        }
        Ok(())
    }
}

impl CoinNetwork {
    fn new() -> Self {
        CoinNetwork{networks: Vec::new()}
    }
}

impl CoinNetwork {
    fn status(&self) -> String {
        let mut msg = String::new();
        for (i, network) in self.networks.iter().enumerate() {
            msg += &format!("{}{}{}", if i != 0 {"\n"} else {""}, network, if i + 1 < self.networks.len() {"\n"} else {""});
        }
        msg
    }
}

// first bool is withdraw, second is deposit
async fn get_avax_asset_status(client: & WithdrawalClient) -> Result<CoinNetwork, String> {
    match client.get_asset_detail().with_recv_window(10000).json::<Vec<Value>>().await {
        Ok(res) => {
            for coin in & res {
                if coin["coin"] == "AVAX" {
                    let mut status = CoinNetwork::new();

                    return match coin["networkList"].as_array() {
                        Some(networks) => {
                            for network in networks {
                                status.networks.push(CoinStatus{network: network["network"].as_str().unwrap().to_string(), deposit: network["depositEnable"].as_bool().unwrap(), deposit_desc: network["depositDesc"].as_str().unwrap().to_string(),
                                    withdraw: network["withdrawEnable"].as_bool().unwrap(), withdraw_desc: network["withdrawDesc"].as_str().unwrap().to_string()});
                            }
                            Ok(status)
                        }
                        None => Err(format!("Error with json parsing {}", "network")),
                    }
                }
            }
            Err("No Avax found".to_string())
        },
        Err(err) => Err(err.to_string())
    }
}

fn add_utc_line(msg: &str) -> String {
    let utc = Utc::now().naive_utc().to_string();
    format!("{}\n{} UTC", msg, utc.split('.').collect::<Vec<&str>>()[0])
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let telegram_bot_token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");
    let telegram_chat_id = env::var("TELEGRAM_CHAT_ID").expect("TELEGRAM_CHAT_ID not set");
    let api_key = env::var("BINANCE_API_KEY").expect("BINANCE_API_KEY not set");
    let secret_key = env::var("BINANCE_SECRET_KEY").expect("BINANCE_SECRET_KEY not set");

    let mut binance_client = WithdrawalClient::connect(&api_key, &secret_key, "https://api.binance.com")?;
    let mut telegram_api = Api::new(&telegram_bot_token);
    let chat = ChatId::new(telegram_chat_id.parse::<i64>()?);

    let mut save_status;
    
    match get_avax_asset_status(&binance_client).await {
        Ok(res) => save_status = res,
        Err(err) => return Err(err.into())
    }

    let mut msg = add_utc_line(&save_status.status());

    if let Err(err) = telegram_api.send(chat.text(&msg)).await {
        eprintln!("Error sending telegram msg {}", err);
        return Err(err.into())
    }
    println!("{}", &msg);
    
    let mut binance_retry: i32 = 0;
    let mut telegram_retry: i32 = 0;

    loop {
        println!("{}", add_utc_line("Send request to binance")); // for debug
        match get_avax_asset_status(&binance_client).await {
            Ok(asset_status) => {
                if save_status != asset_status {
                    msg = add_utc_line(&asset_status.status());
                    println!("{}",msg);
                    if let Err(err) = telegram_api.send_timeout(chat.text(&msg), Duration::from_secs(8)).await {
                        println!("Error sending telegram msg {}", err);
                        telegram_retry += 1;
                    } else {
                        save_status = asset_status;
                        telegram_retry = 0;
                    }
                }
                binance_retry = 0;
            },
            Err(err) => {
                eprintln!("Error binance api {}", err);
                binance_retry += 1;
            }
        }
        if binance_retry == MAX_API_RETRY {
            println!("Too much errors binance api, waiting 1 hour");
            sleep(Duration::from_secs(3600));
            binance_client = WithdrawalClient::connect(&api_key, &secret_key, "https://api.binance.com")?;
        }
        if telegram_retry == MAX_API_RETRY {
            println!("Too much errors telegram api, waiting 1 hour");
            sleep(Duration::from_secs(3600));
            telegram_api = Api::new(&telegram_bot_token);
        }
        sleep(Duration::from_secs(REFRESH_RATE));
    }
}
