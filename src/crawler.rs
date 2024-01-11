use std::{sync::atomic::*, time::Duration};

use sf_api::{
    command::Command,
    gamestate::{character::*, GameState},
    session::*,
};

use crate::CharacterInfo;

pub enum CrawlerCommand {
    Pause,
    Start,
}

pub static PAGE_POS: AtomicUsize = AtomicUsize::new(0);
pub static FETCHED_PLAYERS: AtomicUsize = AtomicUsize::new(0);

pub async fn crawl(
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<CrawlerCommand>,
    out: tokio::sync::mpsc::UnboundedSender<CharacterInfo>,
    mut started: bool,
    pages: Vec<usize>,
    server: ServerConnection,
    username: String,
) {
    let password = username.chars().rev().collect::<String>();
    let (mut session, response) = match CharacterSession::register(
        &username,
        &password,
        server.clone(),
        Gender::Male,
        Race::DarkElf,
        Class::Mage,
    )
    .await
    {
        Ok(x) => x,
        Err(_) => {
            let mut session =
                CharacterSession::new(&username, &password, server);
            let resp = session.login().await.unwrap();
            (session, resp)
        }
    };

    let mut gs = GameState::new(response).unwrap();

    let mut todo_accounts: Vec<String> = Vec::new();

    loop {
        while let Some(todo) = todo_accounts.pop() {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let r = session
                .send_command(&Command::ViewPlayer {
                    ident: todo.clone(),
                })
                .await;

            FETCHED_PLAYERS.fetch_add(1, Ordering::SeqCst);
            if let Ok(resp) = r {
                gs.update(resp).unwrap();
                let Some(player) = gs.other_players.lookup_name(&todo).cloned()
                else {
                    continue;
                };
                let equipment = player
                    .equipment
                    .0
                    .iter()
                    .flatten()
                    .filter_map(|a| a.equipment_ident())
                    .collect();

                out.send(CharacterInfo {
                    equipment,
                    name: player.name,
                    uid: player.player_id,
                    level: player.level,
                })
                .unwrap();
            }
        }

        match receiver.try_recv() {
            Ok(command) => match command {
                CrawlerCommand::Pause => {
                    started = false;
                }
                CrawlerCommand::Start => {
                    started = true;
                }
            },
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return;
            }
        }
        if started {
            // gs.other_players.reset_lookups();
            let pos =
                PAGE_POS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            let Some(page) = pages.get(pos).copied() else {
                // We fetched the entire HoF
                return;
            };

            let Ok(resp) = session
                .send_command(&Command::HallOfFamePage { page })
                .await
            else {
                continue;
            };

            gs.update(resp).unwrap();

            for hof in &gs.other_players.hall_of_fame {
                todo_accounts.push(hof.name.to_string())
            }
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
