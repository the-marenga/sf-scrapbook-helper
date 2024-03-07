use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::Utc;
use sf_api::{
    error::SFError,
    gamestate::{character::*, GameState},
    session::*,
};
use tokio::{sync::RwLock, time::sleep};

use crate::*;

pub struct Crawler {
    pub dead: bool,
    pub que: Arc<Mutex<WorkerQue>>,
    pub state: Arc<CrawlerState>,
    pub server_id: ServerID,
}

impl Crawler {
    pub async fn crawl(&mut self) -> Message {
        if self.dead {
            sleep(Duration::from_secs(60)).await;
            return Message::CrawlerDead;
        }

        let action = {
            // Thi: CrawlActions is in a seperate scope to immediately drop the
            // guard
            let mut lock = self.que.lock().unwrap();
            loop {
                match lock.todo_accounts.pop() {
                    Some(entry) => {
                        if entry.chars().all(|a| a.is_ascii_digit()) {
                            // We will get a wrong result here, because
                            // fetching them will be seen as a request to view
                            // a player by id, not by name
                            lock.invalid_accounts.push(entry);
                            continue;
                        }
                        lock.in_flight_accounts.push(entry.clone());
                        break CrawlAction::Character(entry, lock.que_id);
                    }
                    None => match lock.todo_pages.pop() {
                        Some(idx) => {
                            lock.in_flight_pages.push(idx);
                            break CrawlAction::Page(idx, lock.que_id);
                        }
                        None => break CrawlAction::Wait,
                    },
                }
            }
        };

        use sf_api::command::Command;
        let session = self.state.session.read().await;
        match &action {
            CrawlAction::Wait => {
                drop(session);
                sleep(Duration::from_secs(1)).await;
                Message::CrawlerIdle
            }
            CrawlAction::Page(page, _) => {
                let cmd = Command::HallOfFamePage { page: *page };
                let Ok(resp) = session.send_command_raw(&cmd).await else {
                    return Message::CrawlerUnable {
                        server: self.server_id,
                        action,
                    };
                };
                drop(session);
                let mut gs = self.state.gs.lock().unwrap();
                if gs.update(resp).is_err() {
                    return Message::CrawlerUnable {
                        server: self.server_id,
                        action,
                    };
                };

                let mut lock = self.que.lock().unwrap();
                for acc in gs.other_players.hall_of_fame.drain(..) {
                    lock.todo_accounts.push(acc.name);
                }
                lock.in_flight_pages.retain(|a| a != page);
                Message::PageCrawled
            }
            CrawlAction::Character(name, que_id) => {
                let cmd = Command::ViewPlayer {
                    ident: name.clone(),
                };
                let Ok(resp) = session.send_command_raw(&cmd).await else {
                    return Message::CrawlerUnable {
                        server: self.server_id,
                        action,
                    };
                };
                drop(session);
                let mut gs = self.state.gs.lock().unwrap();
                if gs.update(resp).is_err() {
                    return Message::CrawlerUnable {
                        server: self.server_id,
                        action,
                    };
                }

                let character = match gs.other_players.lookup_name(name) {
                    Some(player) => {
                        let equipment = player
                            .equipment
                            .0
                            .iter()
                            .flatten()
                            .filter_map(|a| a.equipment_ident())
                            .collect();
                        let stats =
                            player.base_attributes.0.iter().sum::<u32>()
                                + player.bonus_attributes.0.iter().sum::<u32>();
                        CharacterInfo {
                            equipment,
                            name: player.name.clone(),
                            uid: player.player_id,
                            level: player.level,
                            fetch_date: Some(Utc::now().date_naive()),
                            stats: Some(stats),
                        }
                    }
                    None => {
                        drop(gs);
                        let mut lock = self.que.lock().unwrap();
                        if lock.que_id == *que_id {
                            lock.invalid_accounts.retain(|a| a != name);
                        }
                        lock.invalid_accounts.push(name.to_string());
                        return Message::CrawlerNoPlayerResult;
                    }
                };
                Message::CharacterCrawled {
                    server: self.server_id,
                    que_id: *que_id,
                    character,
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct CrawlerState {
    pub session: RwLock<CharacterSession>,
    pub gs: Mutex<GameState>,
}
impl CrawlerState {
    pub async fn try_login(
        name: String,
        server: ServerConnection,
    ) -> Result<Self, SFError> {
        let password = name.chars().rev().collect::<String>();
        let mut session =
            CharacterSession::new(&name, &password, server.clone());
        if let Ok(resp) = session.login().await {
            let gs = GameState::new(resp)?;
            return Ok(Self {
                session: RwLock::new(session),
                gs: Mutex::new(gs),
            });
        };

        let all_races = [
            Race::Human,
            Race::Elf,
            Race::Dwarf,
            Race::Gnome,
            Race::Orc,
            Race::DarkElf,
            Race::Goblin,
            Race::Demon,
        ];

        let all_classes = [
            Class::Warrior,
            Class::Mage,
            Class::Scout,
            Class::Assassin,
            Class::BattleMage,
            Class::Berserker,
            Class::DemonHunter,
            Class::Druid,
            Class::Bard,
            Class::Necromancer,
        ];

        let mut rng = fastrand::Rng::new();
        let gender = rng.choice([Gender::Female, Gender::Male]).unwrap();
        let race = rng.choice(all_races).unwrap();
        let class = rng.choice(all_classes).unwrap();

        let (session, resp) = CharacterSession::register(
            &name,
            &password,
            server.clone(),
            gender,
            race,
            class,
        )
        .await?;

        let gs = GameState::new(resp)?;

        Ok(Self {
            session: RwLock::new(session),
            gs: Mutex::new(gs),
        })
    }
}

#[derive(Debug, Clone)]
pub enum CrawlAction {
    Wait,
    Page(usize, QueID),
    Character(String, QueID),
}

#[derive(
    Debug, Serialize, Deserialize, Default, Clone, Copy, PartialEq, Eq,
)]
pub enum CrawlingOrder {
    #[default]
    Random,
    TopDown,
    BottomUp,
}

impl CrawlingOrder {
    pub fn apply_order(&self, todo_pages: &mut [usize]) {
        match self {
            CrawlingOrder::Random => fastrand::shuffle(todo_pages),
            CrawlingOrder::TopDown => {
                todo_pages.sort_by(|a, b| a.cmp(b).reverse());
            }
            CrawlingOrder::BottomUp => todo_pages.sort(),
        }
    }
}

impl ToString for CrawlingOrder {
    fn to_string(&self) -> String {
        match self {
            CrawlingOrder::Random => "Random",
            CrawlingOrder::TopDown => "Top Down",
            CrawlingOrder::BottomUp => "Bottom Up",
        }
        .to_string()
    }
}

#[derive(Debug)]
pub struct WorkerQue {
    pub que_id: QueID,
    pub todo_pages: Vec<usize>,
    pub todo_accounts: Vec<String>,
    pub invalid_pages: Vec<usize>,
    pub invalid_accounts: Vec<String>,
    pub in_flight_pages: Vec<usize>,
    pub in_flight_accounts: Vec<String>,
    pub order: CrawlingOrder,
}
