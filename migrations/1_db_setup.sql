CREATE TABLE IF NOT EXISTS test (
	id serial,
	name TEXT
);

INSERT INTO test(name) VALUES
('hello'),
('world'),
('!')
;

CREATE TABLE IF NOT EXISTS users (
	id INTEGER PRIMARY KEY
	,first_name TEXT
	,last_name TEXT
	,username TEXT
	,language_code TEXT
);

CREATE TABLE IF NOT EXISTS game_modes (
	id SERIAL PRIMARY KEY
	,game_mode TEXT UNIQUE
);

INSERT INTO game_modes(game_mode) VALUES
	('full')
	,('half')
	,('10')
	,('5')

	ON CONFLICT DO NOTHING
;

CREATE TABLE IF NOT EXISTS chats (
	id INTEGER PRIMARY KEY
	,game_mode INTEGER REFERENCES game_modes(id)

);

CREATE TABLE IF NOT EXISTS points(
	id serial PRIMARY KEY
	,chat_id INTEGER REFERENCES chats(id)
	,user_id INTEGER REFERENCES users(id)
	,points INTEGER 
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
);


SET timezone = 'America/New_York';

CREATE TABLE IF NOT EXISTS polls (
	id INTEGER PRIMARY KEY
	,game INTEGER REFERENCES games(id)
);


CREATE TABLE IF NOT EXISTS bets (
	id SERIAL PRIMARY KEY
	,game_id INTEGER REFERENCES games(id)
	,chat_id INTEGER REFERENCES chats(id)
	,user_id INTEGER REFERENCES users(id)
	,bet INTEGER REFERENCES teams(id)
	,poll INTEGER REFERENCES polls(id)
);


INSERT INTO users(id, first_name) VALUES 
	(1, 'A')
	,(2, 'B')
	,(3, 'C')
	,(4, 'D')
	,(5, 'E')

	ON CONFLICT DO NOTHING
	;


INSERT INTO chats VALUES
	(100, 1)
	,(200, 4)
	ON CONFLICT DO NOTHING
	;

INSERT INTO points(chat_id, user_id, points) VALUES 
	(100, 1, 50)
	,(100, 2, 99)
	,(100, 3, 25)
	,(200, 1, 20)
	,(200, 2, 10)
	,(200, 5, 80)
	;
--DROP VIEW rankings;
CREATE OR REPLACE VIEW rankings AS 
	SELECT
		users.id AS user_id
		--,users.first_name AS first_name
		--,users.last_name AS last_name
		--,users.username AS username
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
