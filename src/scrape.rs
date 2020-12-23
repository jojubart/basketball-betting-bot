use chrono::format::ParseError;
use chrono::prelude::*;
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use chrono_tz::US::Eastern;
use scraper::{Html, Selector};
use sqlx::postgres::PgPool;
use std::env;
use time;
//#[tokio::main]
//async fn main() -> Result<(), reqwest::Error> {
use crate::Error;

pub async fn scrape_teams() -> Result<(), Error> {
    let pool = PgPool::connect(&env::var("DATABASE_URL")?).await?;
    let mut resp = reqwest::get("https://www.basketball-reference.com/leagues/NBA_2020.html")
        .await?
        .text()
        .await?;
    let doc = Html::parse_document(&resp);
    let selector = Selector::parse("tr").unwrap();

    for entry in doc.select(&selector).skip(0).take(32) {
        let td = entry.text().collect::<Vec<_>>();
        if vec![8, 9].contains(&td.len()) {
            let name = td[0];
            let srs = td[td.len() - 1].parse::<sqlx::types::BigDecimal>().unwrap();
            let wins = td[td.len() - 7].parse::<i32>().unwrap();
            let losses = td[td.len() - 6].parse::<i32>().unwrap();
            println!("{:?} {:?}", &name, &srs);
            sqlx::query!(
                r#"
                INSERT INTO teams(name,wins,losses,srs) VALUES
                ($1, $2, $3, $4)
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
pub async fn scrape_games() -> Result<(), Error> {
    let pool = PgPool::connect(&env::var("DATABASE_URL")?).await?;

    let dt = DateTime::parse_from_str(
        "Tue, Oct 22, 2019 8:00:00 pm -0500",
        "%a, %b %d, %Y %I:%M:%S %P %z",
    );
    let d = NaiveDate::parse_from_str("Fri, Oct 25, 2019", "%a, %b %d, %Y").unwrap();
    let t = NaiveTime::parse_from_str("7:30", "%H:%M").unwrap();
    let mut resp = reqwest::get("https://www.basketball-reference.com/leagues/NBA_2021_games.html")
        .await?
        .text()
        .await?;
    let doc = Html::parse_document(&resp);
    let selector = Selector::parse("tr").unwrap();
    //let table = doc.select(&selector).next().unwrap();
    for entry in doc.select(&selector).skip(1) {
        let td = entry.text().collect::<Vec<_>>();
        println!("{}", td.len());
        println!("{:?}", td);
        if td.len() == 4 {
            let date = td[0];
            let time = td[1].replace("0p", "0:00 pm -0500");
            let dtg = format!("{} {}", date, time);
            let dt = DateTime::parse_from_str(&dtg, "%a, %b %d, %Y %I:%M:%S %P %z").unwrap();
            let utc_time = dt.with_timezone(&Utc);
            let away_team = td[2].to_string();
            let home_team = td[3].to_string();

            println!("{:?}", dtg);
            //let d = time::OffsetDateTime::parse(&dtg, "%a, %b %d, %Y %I:%M:%S %P %z").unwrap();

            let g = Game {
                date: dt,
                //date: dt,
                away_team,
                home_team,
            };
            println!("Game {:?}", g);
            let away_team_id = sqlx::query!(
                r#"
            SELECT id FROM teams WHERE name = $1"#,
                g.away_team
            )
            .fetch_one(&pool)
            .await
            .expect(format!("Could not get id for team {}", g.away_team).as_str());

            let home_team_id = sqlx::query!(
                r#"
            SELECT id FROM teams WHERE name = $1"#,
                g.home_team
            )
            .fetch_one(&pool)
            .await?;

            //.expect(format!("Could not get id for team {}", home_team).as_str());

            sqlx::query!(
                r#"
        INSERT INTO games(date_time, away_team, home_team)
        VALUES
        ($1, $2, $3)
        ON CONFLICT DO NOTHING;
        "#,
                g.date,
                away_team_id.id,
                home_team_id.id
            )
            .execute(&pool)
            .await;
            println!("dt: {:?}", dt);
            println!("utc_time: {:?}", utc_time);

            //println!("{:?}", td[1].replace("0p", "0:00 pm -0500"));
            for i in td.iter() {
                println!("{}", i);
            }
        }

        println!("{:?}", chrono::offset::Utc::now());
        println!("{:?}", dt.unwrap());
    }
    Ok(())
}
#[derive(Debug)]
struct Game {
    date: DateTime<FixedOffset>,
    //date: time::OffsetDateTime,
    away_team: String,
    home_team: String,
}

#[derive(Debug)]
struct Team {
    name: String,
    srs: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_test() {}
}
