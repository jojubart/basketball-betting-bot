
CREATE TABLE IF NOT EXISTS users (
	id BIGINT PRIMARY KEY
	,first_name TEXT
	,last_name TEXT
	,username TEXT
	,language_code TEXT
);

CREATE TABLE IF NOT EXISTS game_modes (
	id INTEGER PRIMARY KEY
	,game_mode TEXT UNIQUE
	,number_of_games INTEGER
);

INSERT INTO game_modes(id, game_mode, number_of_games) VALUES
	(1, 'full', 1000)
	,(2, 'best_of', 10)

	ON CONFLICT DO NOTHING
;

CREATE TABLE IF NOT EXISTS ranking_systems (
	id INTEGER PRIMARY KEY
	,ranking_system TEXT UNIQUE
);

INSERT INTO ranking_systems(id, ranking_system) VALUES 
	(1, 'weekly')
	,(2, 'per_game')
	ON CONFLICT DO NOTHING
;

CREATE TABLE IF NOT EXISTS chats (
	id BIGINT PRIMARY KEY
	,game_mode INTEGER REFERENCES game_modes(id) DEFAULT 2
	,ranking_system INTEGER REFERENCES ranking_systems(id) DEFAULT 1

);

CREATE TABLE IF NOT EXISTS points(
	id serial PRIMARY KEY
	,chat_id BIGINT REFERENCES chats(id)
	,user_id BIGINT REFERENCES users(id)
	,points INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS teams (
	id serial PRIMARY KEY
	,name TEXT UNIQUE 
	,wins INTEGER DEFAULT 0
	,losses INTEGER DEFAULT 0
	,srs NUMERIC(4,2) DEFAULT 0

);

CREATE TABLE IF NOT EXISTS games (
	id SERIAL PRIMARY KEY
	,date_time TIMESTAMPTZ
	,away_team INTEGER REFERENCES teams(id)
	,away_points INTEGER DEFAULT 0
	,home_team INTEGER REFERENCES teams(id)
	,home_points INTEGER DEFAULT 0 
	,UNIQUE (date_time, away_team, home_team)
);

CREATE TABLE IF NOT EXISTS bet_weeks (
	id SERIAL PRIMARY KEY
	,chat_id BIGINT REFERENCES chats(id)
	,week_number INT
	,start_date DATE
	,end_date DATE
	,polls_sent BOOLEAN DEFAULT False
);

CREATE TABLE IF NOT EXISTS polls (
	id TEXT PRIMARY KEY
	,local_id INT
	,game_id INTEGER REFERENCES games(id)
	,chat_id BIGINT REFERENCES chats(id)
	,is_open BOOLEAN DEFAULT TRUE
	,poll_sent_date DATE
	,bet_week_id INTEGER REFERENCES bet_weeks(id)
	
);


--CREATE TABLE IF NOT EXISTS polls_sent (
--	id SERIAL PRIMARY KEY
--	,chat_id INTEGER REFERENCES chats(id)
--	,poll INTEGER REFERENCES polls(id)
--	,date DATE
--);


CREATE TABLE IF NOT EXISTS bets (
	id SERIAL PRIMARY KEY
	,game_id INTEGER REFERENCES games(id)
	,chat_id BIGINT REFERENCES chats(id)
	,user_id BIGINT REFERENCES users(id)
	,bet INTEGER REFERENCES teams(id)
	,poll_id TEXT REFERENCES polls(id)
);





--DROP VIEW rankings;
CREATE OR REPLACE VIEW rankings AS 
	SELECT
		users.id AS user_id
		,users.first_name AS first_name
		,users.last_name AS last_name
		,users.username AS username
		,chats.id AS chat_id
		,points.points 
		,RANK() OVER (
			PARTITION BY chat_id
			ORDER BY points DESC
		) rank_number
		
	FROM
		users
		JOIN 
		points ON users.id = points.user_id
		JOIN
		chats ON points.chat_id = chats.id
;
			
CREATE OR REPLACE VIEW full_game_information AS
	SELECT 
		games.id AS game_id
		,games.date_time
		,games.away_team AS away_team_id
		,t1.name AS away_team
		--,games.away_points
		,t1.srs AS srs_away
		,games.home_team AS home_team_id
		,t2.name AS home_team
		--,games.home_points
		,t2.srs as srs_home
		,t1.srs + t2.srs AS srs_sum

	FROM games 
	JOIN
	teams AS t1 ON games.away_team = t1.id
	JOIN 
	teams AS t2 ON games.home_team = t2.id

	ORDER BY date_time ASC
;

CREATE OR REPLACE VIEW full_chat_information AS
	SELECT 
		chats.id AS chat_id
		,chats.game_mode AS game_mode_id
		,chats.ranking_system AS ranking_system_id
		,game_modes.game_mode
		,game_modes.number_of_games
		,ranking_systems.ranking_system
	FROM chats
	JOIN
	game_modes ON game_modes.id = chats.game_mode
	JOIN
	ranking_systems ON ranking_systems.id = chats.ranking_system
;

-- nested query necessary to avoid writing out CASE statement again in WHERE clause
CREATE OR REPLACE VIEW game_winners AS
	SELECT * FROM 
	(SELECT
		id AS game_id
		,CASE
			WHEN home_points > away_points THEN home_team
			WHEN home_points < away_points THEN away_team
		END AS winner
		
	FROM games) tmp
	WHERE winner IS NOT NULL;


CREATE OR REPLACE VIEW correct_bets AS
	SELECT
		bets.game_id
		,bets.chat_id
		,bets.user_id
		,bets.bet
		--,polls.chat_id
		,bet_weeks.week_number
		
	FROM bets
	JOIN
		game_winners ON bets.bet = game_winners.winner
	JOIN 
		polls ON bets.poll_id = polls.id
	JOIN bet_weeks ON polls.bet_week_id = bet_weeks.id

	;

CREATE OR REPLACE VIEW weekly_rankings AS
SELECT
	users.id
	,users.first_name
	,users.last_name
	,users.username
	,week_number
	,correct_bets_week
	,RANK() OVER (
			PARTITION BY chat_id, week_number
			ORDER BY correct_bets_week DESC
		) rank_number

FROM
	users
JOIN
	(SELECT
		users.id
		,correct_bets.week_number
		,count(*) AS correct_bets_week
		,chat_id
	FROM correct_bets 
	JOIN 
		users ON correct_bets.user_id = users.id
	GROUP BY 
		users.id
		,chat_id
		,correct_bets.week_number) as tmp
	ON users.id = tmp.id
;

ALTER DATABASE postgres SET timezone TO 'America/New_York';
SELECT pg_reload_conf();

