use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{DateTime, Local};
use log::trace;
use nohash_hasher::IntMap;
use sf_api::{
    gamestate::{unlockables::ScrapBook, GameState},
    session::CharacterSession,
};
use tokio::time::sleep;

use crate::{login::Auth, message::Message, AccountIdent, AttackTarget};

pub struct AccountInfo {
    pub name: String,
    pub ident: AccountIdent,
    pub auth: Auth,
    pub last_updated: DateTime<Local>,
    pub status: Arc<Mutex<AccountStatus>>,
    pub scrapbook_info: Option<ScrapbookInfo>,
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
    pub fn new(gs: &GameState) -> Option<Self> {
        Some(Self {
            scrapbook: gs.unlocks.scrapbok.as_ref()?.clone(),
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
        auth: Auth,
        account_ident: AccountIdent,
    ) -> AccountInfo {
        AccountInfo {
            name: name.to_string(),
            auth,
            scrapbook_info: None,
            last_updated: Local::now(),
            status: Arc::new(Mutex::new(AccountStatus::LoggingIn)),
            ident: account_ident,
        }
    }
}

pub enum AccountStatus {
    LoggingIn,
    Idle(Box<CharacterSession>, Box<GameState>),
    Busy(Box<GameState>),
    FatalError(String),
    LoggingInAgain,
}

impl AccountStatus {
    pub fn take_session(&mut self) -> Option<Box<CharacterSession>> {
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

    pub fn put_session(&mut self, session: Box<CharacterSession>) {
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
        loop {
            sleep(Duration::from_millis(fastrand::u64(5000..=10000))).await;
            let mut session = {
                let mut lock = self.player_status.lock().unwrap();
                let res = lock.take_session();
                match res {
                    Some(res) => res,
                    None => continue,
                }
            };

            trace!("Sending poll {:?}", self.ident);

            let Ok(resp) = session
                .send_command(&sf_api::command::Command::UpdatePlayer)
                .await
            else {
                return Message::PlayerCommandFailed {
                    ident: self.ident,
                    session,
                };
            };
            let mut lock = self.player_status.lock().unwrap();
            let gs = match &mut *lock {
                AccountStatus::Busy(gs) => gs,
                _ => {
                    lock.put_session(session);
                    continue;
                }
            };
            if gs.update(resp).is_err() {
                return Message::PlayerCommandFailed {
                    ident: self.ident,
                    session,
                };
            }
            lock.put_session(session);
        }
    }
}
