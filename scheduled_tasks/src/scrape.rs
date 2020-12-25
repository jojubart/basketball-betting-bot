use chrono::prelude::*;
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use log::warn;
use scraper::{Html, Selector};
use sqlx::postgres::PgPool;
use std::env;
//#[tokio::main]
//async fn main() -> Result<(), reqwest::Error> {
//use crate::Error;
use basketball_betting_bot::Error;

pub async fn scrape_teams() -> Result<(), Error> {
    let pool = PgPool::connect(&env::var("DATABASE_URL")?).await?;
    // in the new year we can use the standings of the actual season, when enough games are played,
    // such thtat the srs metric has significant meaning
    let year = chrono::Utc::now().year();
    let link = format!(
        "https://www.basketball-reference.com/leagues/NBA_{year}.html",
        year = year
    );
    let resp = reqwest::get(&link).await?.text().await?;
    let doc = Html::parse_document(&resp);
    let selector = Selector::parse("tr").unwrap();

    for entry in doc.select(&selector) {
        let td = entry.text().collect::<Vec<_>>();

        // if the row has 8 or 9 entries, it's a team description
        if vec![8, 9, 10].contains(&td.len()) {
            dbg!(&td);
            dbg!(&td.len());
            let name = td[0];
            let srs = td[td.len() - 1].parse::<sqlx::types::BigDecimal>().unwrap();
            let wins = td[td.len() - 7].parse::<i32>().unwrap();
            let losses = td[td.len() - 6].parse::<i32>().unwrap();
            println!("{:?} {:?}", &name, &srs);
            sqlx::query!(
                r#"
                INSERT INTO teams(name,wins,losses,srs) VALUES
                ($1, $2, $3, $4)
                ON CONFLICT (name) DO
                UPDATE SET (wins, losses, srs) = ($2, $3, $4);
            "#,
                name,
                wins,
                losses,
                srs
            )
            .execute(&pool)
            .await;
        }
    }

    Ok(())
}
pub async fn scrape_games(month: String) -> Result<(), Error> {
    let pool = PgPool::connect(&env::var("DATABASE_URL")?).await?;
    let link = format!(
        "https://www.basketball-reference.com/leagues/NBA_2021_games-{month}.html",
        month = month
    )
    .to_string();
    let resp = reqwest::get(&link).await?.text().await?;
    let doc = Html::parse_document(&resp);
    let selector = Selector::parse("tr").unwrap();
    for entry in doc.select(&selector) {
        let td = entry.text().collect::<Vec<_>>();
        let date = td[0];

        let time = td[1].replace("0p", "0:00 pm -0500");
        let dtg = format!("{} {}", date, time);
        let dt = DateTime::parse_from_str(&dtg, "%a, %b %d, %Y %I:%M:%S %P %z");

        // if we can't parse the date it almost certainly means the row is a header and not a game
        let mut skip = false;
        if let Err(e) = dt {
            skip = true;
            warn!("skipped the row with error {}", e);
        }
        if skip {
            continue;
        }

        let away_team = td[2].to_string();

        // if row has 4 entries, the game was not played yet
        match td.len() {
            4 => {
                let home_team = td[3].to_string();

                let game = Game {
                    date: dt?,
                    away_team,
                    home_team,
                };

                let away_team_id = get_team_id(&pool, game.away_team).await?;
                let home_team_id = get_team_id(&pool, game.home_team).await?;

                add_game(&pool, game.date, away_team_id, 0, home_team_id, 0).await?;
            }
            _ => {
                let home_team = td[4].to_string();
                let home_points: i32 = td[5].parse().unwrap();
                let away_points: i32 = td[3].parse().unwrap();

                let game = FinishedGame {
                    date: dt?,
                    away_team,
                    away_points,
                    home_team,
                    home_points,
                };
                let away_team_id = get_team_id(&pool, game.away_team).await?;
                let home_team_id = get_team_id(&pool, game.home_team).await?;

                add_game(
                    &pool,
                    game.date,
                    away_team_id,
                    game.away_points,
                    home_team_id,
                    game.home_points,
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn add_game(
    pool: &PgPool,
    date_time: DateTime<FixedOffset>,
    away_team_id: i32,
    away_points: i32,
    home_team_id: i32,
    home_points: i32,
) -> Result<(), Error> {
    sqlx::query!(
        r#"
        INSERT INTO games(date_time, away_team, away_points, home_team, home_points)
        VALUES
        ($1, $2, $3, $4, $5)
        ON CONFLICT (date_time, away_team, home_team) DO
            UPDATE SET (date_time, away_points, home_points) = ($1, $3, $5);
        "#,
        date_time,
        away_team_id,
        away_points,
        home_team_id,
        home_points
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn get_team_id(pool: &PgPool, team_name: String) -> Result<i32, Error> {
    Ok(sqlx::query!(
        r#"
            SELECT id FROM teams WHERE name = $1"#,
        team_name
    )
    .fetch_optional(pool)
    .await?
    .unwrap()
    .id)
}
#[derive(Debug, Clone)]
struct Game {
    date: DateTime<FixedOffset>,
    //date: time::OffsetDateTime,
    away_team: String,
    home_team: String,
}

#[derive(Debug, Clone)]
struct FinishedGame {
    date: DateTime<FixedOffset>,
    away_team: String,
    away_points: i32,
    home_team: String,
    home_points: i32,
}

#[derive(Debug)]
struct Team {
    name: String,
    srs: f32,
}
