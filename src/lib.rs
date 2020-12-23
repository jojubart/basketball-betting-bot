//pub mod main;
pub mod scrape;

#[macro_use]
extern crate derive_more;
use ini::Ini;
use teloxide::RequestError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("error from Telegram: {0}")]
    TelegramError(#[from] RequestError),
    #[error("error from SQLx: {0}")]
    SQLxError(#[from] sqlx::Error),
    #[error("error from std::env: {0}")]
    EnvError(#[from] std::env::VarError),
    #[error("error from reqwest: {0}")]
    ReqwestError(#[from] reqwest::Error),
    #[error("error from chrono: {0}")]
    ChronoError(#[from] chrono::ParseError),
}

pub fn get_token(file_location: &str) -> String {
    let conf = Ini::load_from_file(file_location).expect("No .ini file found!");
    let token = conf
        .section(Some("Bot"))
        .expect("There is not 'Bot' Section in the .ini file!")
        .get("token")
        .expect("No token found!");
    token.to_string()
}

pub fn east_coast_date_today() -> Result<chrono::NaiveDate, Error> {
    let today_east_coast_delayed_format = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::hours(5))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();

    Ok(chrono::NaiveDate::parse_from_str(
        &today_east_coast_delayed_format,
        "%Y-%m-%d",
    )?)
}

/// past: describes if you want the day x days in the past (true) or in the future (false)
pub fn east_coast_date_in_x_days(days: i64, past: bool) -> Result<chrono::NaiveDate, Error> {
    let east_coast_datetime = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::hours(5))
        .unwrap();
    let east_coast_delayed_format = match past {
        true => east_coast_datetime
            .checked_sub_signed(chrono::Duration::days(days))
            .unwrap()
            .format("%Y-%m-%d")
            .to_string(),
        false => east_coast_datetime
            .checked_add_signed(chrono::Duration::days(days))
            .unwrap()
            .format("%Y-%m-%d")
            .to_string(),
    };

    Ok(chrono::NaiveDate::parse_from_str(
        &east_coast_delayed_format,
        "%Y-%m-%d",
    )?)
}
