use std::{
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex,
    },
    time::Duration,
};

use sf_api::{
    command::Command, gamestate::GameState, session::CharacterSession,
};

use crate::CONTEXT;

pub enum PlayerInfo {
    Victory { name: String, uid: u32 },
    Lost { name: String },
}

pub enum PlayerCommand {
    Attack { name: String, uid: u32, mush: bool },
}

pub async fn handle_player(
    output: Sender<PlayerInfo>,
    receiver: Receiver<PlayerCommand>,
    mut session: CharacterSession,
    gs: Arc<Mutex<GameState>>,
) {
    loop {
        let Ok(cmd) = receiver.try_recv() else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };

        match cmd {
            PlayerCommand::Attack { name, uid, mush } => {
                if !mush {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }

                for i in 0..2 {
                    if i > 0 {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        println!("Logging in again");
                        let resp1 = session.login().await.unwrap();
                        let resp2 = session
                            .send_command(&Command::UpdatePlayer)
                            .await
                            .unwrap();
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        gs.lock().unwrap().update(resp1).unwrap();
                        gs.lock().unwrap().update(resp2).unwrap();
                        let c = CONTEXT.get().unwrap();
                        c.request_repaint();
                    }

                    let res = session
                        .send_command(&Command::Fight {
                            name: name.clone(),
                            use_mushroom: mush,
                        })
                        .await;

                    let resp = match res {
                        Ok(x) => x,
                        Err(err) => {
                            println!("Error: {err}");
                            continue;
                        }
                    };

                    let mut gs = gs.lock().unwrap();
                    gs.update(resp).unwrap();

                    let Some(fight) = &gs.last_fight else {
                        println!("No fight");
                        continue;
                    };
                    if fight.has_player_won {
                        output.send(PlayerInfo::Victory { name, uid }).unwrap();
                    } else {
                        output.send(PlayerInfo::Lost { name }).unwrap();
                    }
                    let c = CONTEXT.get().unwrap();
                    c.request_repaint();
                    break;
                }

                while receiver.try_recv().is_ok() {}
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
