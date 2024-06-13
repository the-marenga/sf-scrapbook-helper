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

use self::backup::ZHofBackup;
use crate::*;

pub struct Crawler {
    pub que: Arc<Mutex<WorkerQue>>,
    pub state: Arc<CrawlerState>,
    pub server_id: ServerID,
}

impl Crawler {
    pub async fn crawl(&mut self) -> Message {
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
                        lock.in_flight_accounts.insert(entry.clone());
                        break CrawlAction::Character(entry, lock.que_id);
                    }
                    None => match lock.todo_pages.pop() {
                        Some(idx) => {
                            lock.in_flight_pages.push(idx);
                            break CrawlAction::Page(idx, lock.que_id);
                        }
                        None => {
                            if lock.self_init {
                                lock.self_init = false;
                                break CrawlAction::InitTodo;
                            } else {
                                break CrawlAction::Wait;
                            }
                        }
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
                Message::CrawlerIdle(self.server_id)
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
                for acc in gs.hall_of_fames.players.drain(..) {
                    if acc.level > lock.max_level || acc.level < lock.min_level
                    {
                        match lock.lvl_skipped_accounts.entry(acc.level) {
                            std::collections::btree_map::Entry::Vacant(vac) => {
                                vac.insert(vec![acc.name]);
                            }
                            std::collections::btree_map::Entry::Occupied(
                                mut occ,
                            ) => occ.get_mut().push(acc.name),
                        }
                    } else {
                        lock.todo_accounts.push(acc.name);
                    }
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
                if gs.update(&resp).is_err() {
                    return Message::CrawlerUnable {
                        server: self.server_id,
                        action,
                    };
                }

                let character = match gs.lookup.remove_name(name) {
                    Some(player) => {
                        let equipment = player
                            .equipment
                            .0
                            .as_array()
                            .iter()
                            .flatten()
                            .filter_map(|a| a.equipment_ident())
                            .collect();
                        let stats = player
                            .base_attributes
                            .as_array()
                            .iter()
                            .sum::<u32>()
                            + player
                                .bonus_attributes
                                .as_array()
                                .iter()
                                .sum::<u32>();
                        CharacterInfo {
                            equipment,
                            name: player.name,
                            uid: player.player_id,
                            level: player.level,
                            fetch_date: Some(Utc::now().date_naive()),
                            stats: Some(stats),
                            class: Some(player.class),
                        }
                    }
                    None => {
                        drop(gs);
                        let mut lock = self.que.lock().unwrap();
                        if lock.que_id == *que_id {
                            lock.invalid_accounts.retain(|a| a != name);
                            lock.in_flight_accounts.remove(name);
                            lock.invalid_accounts.push(name.to_string());
                        }
                        return Message::CrawlerNoPlayerResult;
                    }
                };
                Message::CharacterCrawled {
                    server: self.server_id,
                    que_id: *que_id,
                    character,
                }
            }
            CrawlAction::InitTodo => {
                drop(session);
                let gs = self.state.gs.lock().unwrap();
                let pages = (gs.hall_of_fames.players_total as usize)
                    .div_ceil(PER_PAGE);
                drop(gs);
                let mut que = self.que.lock().unwrap();
                que.todo_pages = (0..pages).collect();
                let order = que.order;
                order.apply_order(&mut que.todo_pages);
                Message::CrawlerIdle(self.server_id)
            }
        }
    }
}

#[derive(Debug)]
pub struct CrawlerState {
    pub session: RwLock<Session>,
    pub gs: Mutex<GameState>,
}
impl CrawlerState {
    pub async fn try_login(
        name: String,
        server: ServerConnection,
    ) -> Result<Self, SFError> {
        let password = name.chars().rev().collect::<String>();
        let mut session = Session::new(&name, &password, server.clone());
        debug!("Logging in {name} on {}", session.server_url());
        if let Ok(resp) = session.login().await {
            debug!("Successfully logged in {name} on {}", session.server_url());
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
        debug!(
            "Registering new crawler account {name} on {}",
            session.server_url()
        );

        let (session, resp) = Session::register(
            &name,
            &password,
            server.clone(),
            gender,
            race,
            class,
        )
        .await?;
        let gs = GameState::new(resp)?;

        debug!("Registered {name} successfull {}", session.server_url());

        Ok(Self {
            session: RwLock::new(session),
            gs: Mutex::new(gs),
        })
    }
}

#[derive(Debug, Clone)]
pub enum CrawlAction {
    Wait,
    InitTodo,
    Page(usize, QueID),
    Character(String, QueID),
}

impl std::fmt::Display for CrawlAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrawlAction::Wait => f.write_str("Waiting"),
            CrawlAction::InitTodo => f.write_str("Inititialization"),
            CrawlAction::Page(page, _) => {
                f.write_fmt(format_args!("Fetch page {page}"))
            }
            CrawlAction::Character(name, _) => {
                f.write_fmt(format_args!("Fetch char {name}"))
            }
        }
    }
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

#[allow(clippy::to_string_trait_impl)]
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
    pub in_flight_accounts: HashSet<String>,
    pub order: CrawlingOrder,
    pub lvl_skipped_accounts: BTreeMap<u32, Vec<String>>,
    pub min_level: u32,
    pub max_level: u32,
    pub self_init: bool,
}

impl WorkerQue {
    pub fn create_backup(
        &self,
        player_info: &IntMap<u32, CharacterInfo>,
    ) -> ZHofBackup {
        let mut backup = ZHofBackup {
            todo_pages: self.todo_pages.to_owned(),
            invalid_pages: self.invalid_pages.to_owned(),
            todo_accounts: self.todo_accounts.to_owned(),
            invalid_accounts: self.invalid_accounts.to_owned(),
            order: self.order,
            export_time: Some(Utc::now()),
            characters: player_info.values().cloned().collect(),
            lvl_skipped_accounts: self.lvl_skipped_accounts.clone(),
            min_level: self.min_level,
            max_level: self.max_level,
        };

        for acc in &self.in_flight_accounts {
            backup.todo_accounts.push(acc.to_string())
        }

        for page in &self.in_flight_pages {
            backup.todo_pages.push(*page)
        }

        backup
    }

    pub fn count_remaining(&self) -> usize {
        self.todo_pages.len() * PER_PAGE
            + self.todo_accounts.len()
            + self.in_flight_pages.len() * PER_PAGE
            + self.in_flight_accounts.len()
    }
}
