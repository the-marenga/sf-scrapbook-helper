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
    config::CharacterConfig, login::PlayerAuth, message::Message, AccountIdent,
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
    pub auto_lure: bool,
}

impl UnderworldInfo {
    pub fn new(
        gs: &GameState,
        config: Option<&CharacterConfig>,
    ) -> Option<Self> {
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
            auto_lure: config.map(|a| a.auto_lure).unwrap_or(false),
        })
    }
}

pub struct ScrapbookInfo {
    pub scrapbook: ScrapBook,
    pub best: Vec<AttackTarget>,
    pub max_level: u16,
    pub max_attributes: u32,
    pub blacklist: IntMap<u32, (String, usize)>,
    pub attack_log: Vec<(DateTime<Local>, AttackTarget, bool)>,
    pub auto_battle: bool,
}

impl ScrapbookInfo {
    const DEFAULT_ATTRIBUTE_FACTOR: f32 = 1.2;
    pub fn new(
        gs: &GameState,
        config: Option<&CharacterConfig>,
    ) -> Option<Self> {
        let total_attributes = gs.character.attribute_basis.as_array().iter().sum::<u32>()
            + gs.character.attribute_additions.as_array().iter().sum::<u32>();
        Some(Self {
            scrapbook: gs.character.scrapbok.as_ref()?.clone(),
            best: Default::default(),
            max_level: gs.character.level,
            max_attributes: (total_attributes as f32 * Self::DEFAULT_ATTRIBUTE_FACTOR) as u32,
            blacklist: Default::default(),
            attack_log: Default::default(),
            auto_battle: config.map(|a| a.auto_battle).unwrap_or(false),
        })
    }
}

impl AccountInfo {
    pub fn new(
        name: &str,
        auth: PlayerAuth,
        ident: AccountIdent,
    ) -> AccountInfo {
        AccountInfo {
            name: name.to_string(),
            auth,
            scrapbook_info: None,
            underworld_info: None,
            last_updated: Local::now(),
            status: Arc::new(Mutex::new(AccountStatus::LoggingIn)),
            ident,
        }
    }
}

pub enum AccountStatus {
    LoggingIn,
    Idle(Box<Session>, Box<GameState>),
    Busy(Box<GameState>, Box<str>),
    FatalError(String),
    LoggingInAgain,
}

impl AccountStatus {
    pub fn take_session<T: Into<Box<str>>>(
        &mut self,
        reason: T,
    ) -> Option<Box<Session>> {
        let mut res = None;
        *self = match std::mem::replace(self, AccountStatus::LoggingInAgain) {
            AccountStatus::Idle(a, b) => {
                res = Some(a);
                AccountStatus::Busy(b, reason.into())
            }
            x => x,
        };
        res
    }

    pub fn put_session(&mut self, session: Box<Session>) {
        *self = match std::mem::replace(self, AccountStatus::LoggingInAgain) {
            AccountStatus::Busy(a, _) => AccountStatus::Idle(session, a),
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
        tokio::time::sleep(Duration::from_millis(fastrand::u64(1000..=3000)))
            .await;

        Message::AutoBattlePossible { ident: self.ident }
    }
}

pub struct AutoLureChecker {
    pub player_status: Arc<Mutex<AccountStatus>>,
    pub ident: AccountIdent,
}

impl AutoLureChecker {
    pub async fn check(&self) -> Message {
        let lured = {
            match &*self.player_status.lock().unwrap() {
                AccountStatus::Idle(_, session) => {
                    session.underworld.as_ref().map(|a| a.lured_today)
                }
                _ => None,
            }
        };
        let Some(0..=4) = lured else {
            // Either no underworld, or already lured the max
            tokio::time::sleep(Duration::from_millis(fastrand::u64(
                5000..=10_000,
            )))
            .await;
            return Message::AutoLureIdle;
        };

        tokio::time::sleep(Duration::from_millis(fastrand::u64(3000..=5000)))
            .await;

        Message::AutoLurePossible { ident: self.ident }
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
            let res = lock.take_session("Auto Poll");
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
            AccountStatus::Busy(gs, _) => gs,
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
