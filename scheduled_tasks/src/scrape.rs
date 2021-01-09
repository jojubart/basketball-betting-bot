use basketball_betting_bot::utils::set_last_updated;
use basketball_betting_bot::Error;
use chrono::{DateTime, FixedOffset};
use log::warn;
use scraper::{Html, Selector};
use select::document::Document;
use select::predicate::Class;
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::prelude::*;

pub async fn scrape_teams() -> Result<(), Error> {
    let pool = PgPool::connect(&env::var("DATABASE_URL")?).await?;
    // in the new year we can use the standings of the actual season, when enough games are played,
    // such thtat the srs metric has significant meaning
    //let year = chrono::Utc::now().year();
    let year = 2021;
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
            let name = td[0];
            let srs = td[td.len() - 1].parse::<sqlx::types::BigDecimal>().unwrap();
            let wins = td[td.len() - 7].parse::<i32>().unwrap();
            let losses = td[td.len() - 6].parse::<i32>().unwrap();
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
            .await
            .unwrap_or_default();
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
                let home_points: i32 = td[5].parse().unwrap_or(0);
                let away_points: i32 = td[3].parse().unwrap_or(0);

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

pub async fn scrape_games_live(pool: &PgPool) -> anyhow::Result<()> {
    let link = String::from("https://www.covers.com/sports/nba/matchups");
    let full_names = [
        "Atlanta Hawks".to_string(),
        "Boston Celtics".to_string(),
        "Brooklyn Nets".to_string(),
        "Charlotte Hornets".to_string(),
        "Chicago Bulls".to_string(),
        "Cleveland Cavaliers".to_string(),
        "Dallas Mavericks".to_string(),
        "Denver Nuggets".to_string(),
        "Detroit Pistons".to_string(),
        "Golden State Warriors".to_string(),
        "Houston Rockets".to_string(),
        "Indiana Pacers".to_string(),
        "Los Angeles Clippers".to_string(),
        "Los Angeles Lakers".to_string(),
        "Memphis Grizzlies".to_string(),
        "Miami Heat".to_string(),
        "Milwaukee Bucks".to_string(),
        "Minnesota Timberwolves".to_string(),
        "New Orleans Pelicans".to_string(),
        "New York Knicks".to_string(),
        "Oklahoma City Thunder".to_string(),
        "Orlando Magic".to_string(),
        "Philadelphia 76ers".to_string(),
        "Phoenix Suns".to_string(),
        "Portland Trail Blazers".to_string(),
        "Sacramento Kings".to_string(),
        "San Antonio Spurs".to_string(),
        "Toronto Raptors".to_string(),
        "Utah Jazz".to_string(),
        "Washington Wizards".to_string(),
    ];

    let short_names = [
        "ATL".to_string(),
        "BOS".to_string(),
        "BK".to_string(),
        "CHA".to_string(),
        "CHI".to_string(),
        "CLE".to_string(),
        "DAL".to_string(),
        "DEN".to_string(),
        "DET".to_string(),
        "GS".to_string(),
        "HOU".to_string(),
        "IND".to_string(),
        "LAC".to_string(),
        "LAL".to_string(),
        "MEM".to_string(),
        "MIA".to_string(),
        "MIL".to_string(),
        "MIN".to_string(),
        "NO".to_string(),
        "NY".to_string(),
        "OKC".to_string(),
        "ORL".to_string(),
        "PHI".to_string(),
        "PHO".to_string(),
        "POR".to_string(),
        "SAC".to_string(),
        "SA".to_string(),
        "TOR".to_string(),
        "UTA".to_string(),
        "WAS".to_string(),
    ];
    let short_name_to_full_name: HashMap<_, _> =
        short_names.iter().zip(full_names.iter()).collect();
    //let resp = reqwest::blocking::get(&link).unwrap();

    // Document::document::from_read expects type that implements read::io trait
    // In blocking version of reqwest, the Response type implement that trait but in the
    // async version, Response does not. To still be able to use this function without
    // creating a different blocking runtime with reqwest, which does not work anyway,
    // I surrendered and write the reqwest response to file and read from it immediately
    // Sigh...
    let resp = reqwest::get(&link).await?.text().await?;
    let mut file = File::create("scrape.html")?;
    file.write_all(resp.as_bytes()).unwrap();
    let path = std::path::Path::new("scrape.html");
    let file = File::open(path).unwrap();
    let document = Document::from_read(file).unwrap();

    for node in document.find(Class("cmg_game_data")) {
        let home_points = node.attr("data-home-score");
        if home_points.is_none() {
            continue;
        }
        let home_points = home_points.unwrap_or("0").parse::<i32>()?;
        let away_points = node.attr("data-away-score").unwrap_or("0").parse::<i32>()?;
        //2021-01-05T22:47:51.0000000
        let last_updated = chrono::DateTime::parse_from_rfc3339(&format!(
            "{date}{offset}",
            date = node
                .attr("data-last-update")
                .unwrap_or("2000-01-01T00:00:00"),
            offset = "-04:00"
        ))?;
        let game_date = chrono::DateTime::parse_from_str(
            &format!(
                "{date}{offset}",
                date = node.attr("data-game-date").unwrap_or("2000-01-01 00:00:00"),
                offset = "-0500"
            ),
            "%Y-%m-%d %H:%M:%S %z",
        )
        .unwrap();
        let home_team_short = node
            .attr("data-home-team-shortname-search")
            .unwrap()
            .to_string();
        let away_team_short = node
            .attr("data-away-team-shortname-search")
            .unwrap()
            .to_string();

        dbg!(
            &home_points,
            &away_points,
            &last_updated,
            &game_date,
            &home_team_short,
            short_name_to_full_name[&home_team_short],
            &away_team_short,
            short_name_to_full_name[&away_team_short]
        );

        let game = FinishedGame {
            date: game_date,
            away_team: short_name_to_full_name[&away_team_short].to_owned(),
            away_points,
            home_team: short_name_to_full_name[&home_team_short].to_owned(),
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

        set_last_updated(last_updated)?;
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
