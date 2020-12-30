use std::{env, thread::sleep, time::Duration};

use telegram_bot::*;

use tokio_binance::WithdrawalClient;
use serde_json::Value;

use chrono::Utc;

// first bool is withdraw, second is deposit
async fn get_avax_asset_status(client: & WithdrawalClient) -> Result<(bool, bool), String> {
    match client.get_asset_detail().with_recv_window(10000).json::<Value>().await {
        Ok(res) => {
            return match res["assetDetail"]["AVAX"]["withdrawStatus"].as_bool(){
                Some(withdraw_status) => match res["assetDetail"]["AVAX"]["depositStatus"].as_bool() {
                    Some(deposit_status) => Ok((withdraw_status, deposit_status)),
                    None => Err(format!("Error with json parsing {}", res))
                },
                None => Err(format!("Error with json parsing {}", res))
            }
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

    let telegram_api = Api::new(telegram_bot_token);
    
    let client = WithdrawalClient::connect(api_key, secret_key, "https://api.binance.com")?;
    
    let chat = ChatId::new(telegram_chat_id.parse::<i64>()?);
    let mut save_status;
    
    match get_avax_asset_status(&client).await {
        Ok(res) => save_status = res,
        Err(err) => return Err(err.into())
    }

    let mut msg = add_utc_line(&format!("Current Withdrawal status is {}\nCurrent Deposit status is {}", if save_status.0 {"[AVAILABLE]"} else {"[SUSPENDED]"}, if save_status.1 {"[AVAILABLE]"} else {"[SUSPENDED]"}));
    
    if let Err(err) = telegram_api.send(chat.text(&msg)).await {
        eprintln!("Error sending telegram msg {}", err);
        return Err(err.into())
    }
    println!("{}", &msg);
    
    let mut binance_retry: i32 = 0;
    let mut telegram_retry: i32 = 0;
    
    loop {
        println!("{}", add_utc_line("Send request to binance")); // for debug
        match get_avax_asset_status(&client).await {
            Ok(asset_status) => {
                
                if save_status != asset_status {
                    msg = String::from("");
                    if save_status.0 != asset_status.0 {
                        msg.push_str("Withdrawal ");
                        match asset_status.0 {
                            true => msg.push_str("[RESUMED]"),
                            false => msg.push_str("[SUSPENDED]")
                        }
                    }
                    if save_status.1 != asset_status.1 {
                        if !msg.is_empty() {
                            msg.push('\n');
                        }
                        msg.push_str("Deposit ");
                        match asset_status.1 {
                            true => msg.push_str("[RESUMED]"),
                            false => msg.push_str("[SUSPENDED]")
                        }
                    }
                    msg = add_utc_line(&msg);
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
        if binance_retry == 5 {
            println!("Too much errors binance api, waiting 1 hour");
            sleep(Duration::from_secs(3600));
        }
        if telegram_retry == 5 {
            println!("Too much errors telegram api, waiting 1 hour");
            sleep(Duration::from_secs(3600));
        }
        sleep(Duration::from_secs(60));
    }
}
