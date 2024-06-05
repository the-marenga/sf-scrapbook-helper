use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{DateTime, Local};
use log::trace;
use nohash_hasher::IntMap;
use sf_api::{
    gamestate::{underworld::Underworld, unlockables::ScrapBook, GameState},
    session::Session,
};
use tokio::time::sleep;

use crate::{
    config::Config, login::PlayerAuth, message::Message, AccountIdent,
    AttackTarget, CharacterInfo,
};

pub struct AccountInfo {
    pub name: String,
    pub ident: AccountIdent,
    pub auth: PlayerAuth,
    pub last_updated: DateTime<Local>,
    pub status: Arc<Mutex<AccountStatus>>,
    pub scrapbook_info: Option<ScrapbookInfo>,
    pub underworld_info: Option<UnderworldInfo>,
}

pub struct UnderworldInfo {
    pub underworld: Underworld,
    pub best: Vec<CharacterInfo>,
    pub max_level: u16,
    pub attack_log: Vec<(DateTime<Local>, String, bool)>,
}

impl UnderworldInfo {
    pub fn new(gs: &GameState) -> Option<Self> {
        let underworld = gs.underworld.as_ref()?.clone();
        let avg_lvl = underworld
            .units
            .as_array()
            .iter()
            .map(|a| a.level as u64)
            .sum::<u64>() as f32
            / 3.0;
        Some(Self {
            underworld,
            best: Default::default(),
            max_level: avg_lvl as u16 + 20,
            attack_log: Vec::new(),
        })
    }
}

pub struct ScrapbookInfo {
    pub scrapbook: ScrapBook,
    pub best: Vec<AttackTarget>,
    pub max_level: u16,
    pub blacklist: IntMap<u32, (String, usize)>,
    pub attack_log: Vec<(DateTime<Local>, AttackTarget, bool)>,
    pub auto_battle: bool,
}

impl ScrapbookInfo {
    pub fn new(gs: &GameState, _config: &Config) -> Option<Self> {
        Some(Self {
            scrapbook: gs.character.scrapbok.as_ref()?.clone(),
            best: Default::default(),
            max_level: gs.character.level,
            blacklist: Default::default(),
            attack_log: Default::default(),
            auto_battle: false,
        })
    }
}

impl AccountInfo {
    pub fn new(
        name: &str,
        auth: PlayerAuth,
        account_ident: AccountIdent,
        _config: &Config,
    ) -> AccountInfo {
        AccountInfo {
            name: name.to_string(),
            auth,
            scrapbook_info: None,
            underworld_info: None,
            last_updated: Local::now(),
            status: Arc::new(Mutex::new(AccountStatus::LoggingIn)),
            ident: account_ident,
        }
    }
}

pub enum AccountStatus {
    LoggingIn,
    Idle(Box<Session>, Box<GameState>),
    Busy(Box<GameState>),
    FatalError(String),
    LoggingInAgain,
}

impl AccountStatus {
    pub fn take_session(&mut self) -> Option<Box<Session>> {
        let mut res = None;
        *self = match std::mem::replace(self, AccountStatus::LoggingIn) {
            AccountStatus::Idle(a, b) => {
                res = Some(a);
                AccountStatus::Busy(b)
            }
            x => x,
        };
        res
    }

    pub fn put_session(&mut self, session: Box<Session>) {
        *self = match std::mem::replace(self, AccountStatus::LoggingIn) {
            AccountStatus::Busy(a) => AccountStatus::Idle(session, a),
            x => x,
        };
    }
}

pub struct AutoAttackChecker {
    pub player_status: Arc<Mutex<AccountStatus>>,
    pub ident: AccountIdent,
}

impl AutoAttackChecker {
    pub async fn check(&self) -> Message {
        let next_fight: Option<DateTime<Local>> = {
            match &*self.player_status.lock().unwrap() {
                AccountStatus::Idle(_, session) => {
                    session.arena.next_free_fight
                }
                _ => None,
            }
        };
        if let Some(next) = next_fight {
            let remaining = next - Local::now();
            if let Ok(remaining) = remaining.to_std() {
                tokio::time::sleep(remaining).await;
            }
        };
        tokio::time::sleep(Duration::from_millis(fastrand::u64(500..=3000)))
            .await;

        Message::AutoFightPossible { ident: self.ident }
    }
}

pub struct AutoPoll {
    pub player_status: Arc<Mutex<AccountStatus>>,
    pub ident: AccountIdent,
}

impl AutoPoll {
    pub async fn check(&self) -> Message {
        sleep(Duration::from_millis(fastrand::u64(5000..=10000))).await;
        let mut session = {
            let mut lock = self.player_status.lock().unwrap();
            let res = lock.take_session();
            match res {
                Some(res) => res,
                None => return Message::PlayerNotPolled { ident: self.ident },
            }
        };

        trace!("Sending poll {:?}", self.ident);

        let Ok(resp) = session
            .send_command(&sf_api::command::Command::Update)
            .await
        else {
            return Message::PlayerCommandFailed {
                ident: self.ident,
                session,
                attempt: 0,
            };
        };
        let mut lock = self.player_status.lock().unwrap();
        let gs = match &mut *lock {
            AccountStatus::Busy(gs) => gs,
            _ => {
                lock.put_session(session);
                return Message::PlayerNotPolled { ident: self.ident };
            }
        };
        if gs.update(resp).is_err() {
            return Message::PlayerCommandFailed {
                ident: self.ident,
                session,
                attempt: 0,
            };
        }
        lock.put_session(session);
        Message::PlayerPolled { ident: self.ident }
    }
}
