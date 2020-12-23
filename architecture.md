1. At launch, the bot checks the chat_id
	- unknown chat_id?
		- add chat_id to sqlite db
		- SetupState
	- known chat_id?
		- ReadyState
2. Monday X o'clock?
	- announce winner of the current week
	- send out current user standings
	- calculate the games of the week for the chat ->
	use BBallRef's Simple Rating System (SRS) to determine best matchups of the week
	```sql
	select * from full_game_information where date_time < (now() + interval '7 days') ORDER BY srs_sum desc;  
	```
	- send out polls for each game
	- ReadyState
3. Tuesday X o'clock?
	- end polls
	- collect the poll results
	- add everybody who answered poll for first time to user DB
	- ReadyState
4. Everyday X o'clock? 
	- catch results
	- update the user standings
	- update teams table -> wins, losses, SRS
	- ReadyState



**SetupState**
- if it's a group chat:
	1. Greet everybody & ask group admin setup questions:
	2. What's your time zone? (give several options with example cities) (#priority 2)
	3. How many games/week do you want to bet on? 
		- all games
		- ~ half the games
		- ~ 10 games/week
		- ~ 5 games/week
	4. Are there teams that any teams that you want to bet on every time? (#priority 2)
		- yes: 
			- show possible teams
			- ask if there are more he wants to add
		- no: continue
	5. How do you want to rank the participants? 
		- 1 point per succesfull tip => the one with the most correct guesses wins the season
		- only the one with the most correct tips/week gets the point => the one who won the most weeks wins
	~~6. Is everybody who wants to participate in the group right now?~~ (it's not possible to access all user_ids of a group)
	6. 
		- "YOUR BETTING SEASON BEGINS NOW"
		- send out the first polls



- if it's not a group chat (#priority 2):
	- ask user if he really wants to make the bets by himself
		- yes: add his user_id to sqlite
		- same steps as group chat (?!)

**ReadyState**
- /help: show all commands
- /standings: shows table with points/user
- /timezone: change timezone (#priority 2)
- /end:
	- if admin: ask if you want to end the season, this decision is final
		- yes: don't send out polls anymore and send the final results

