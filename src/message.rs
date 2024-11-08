use std::{fmt::Write, sync::Arc, time::Duration};

use chrono::Local;
use config::{CharacterConfig, SFAccCharacter, SFCharIdent};
use crawler::CrawlerError;
use iced::Command;
use log::{error, trace, warn};
use sf_api::{
    gamestate::GameState,
    session::{PWHash, Response, Session},
    sso::SSOProvider,
};
use tokio::time::sleep;
use ui::OverviewAction;

use self::{
    backup::{get_newest_backup, restore_backup, RestoreData},
    login::{SSOIdent, SSOLogin, SSOLoginStatus},
    ui::underworld::LureTarget,
};
use crate::{
    crawler::CrawlerState,
    player::{ScrapbookInfo, UnderworldInfo},
    *,
};

#[derive(Debug, Clone)]
pub enum Message {
    MultiAction {
        action: OverviewAction,
    },
    FontLoaded(Result<(), iced::font::Error>),
    CrawlAllRes {
        servers: Option<Vec<String>>,
        concurrency: usize,
    },
    NextCLICrawling,
    AdvancedLevelRestrict(bool),
    ShowClasses(bool),
    CrawlerSetMinMax {
        server: ServerID,
        min: u32,
        max: u32,
    },
    UpdateResult(bool),
    PlayerSetMaxUndergroundLvl {
        ident: AccountIdent,
        lvl: u16,
    },
    PlayerNotPolled {
        ident: AccountIdent,
    },
    PlayerPolled {
        ident: AccountIdent,
    },
    SetOverviewSelected {
        ident: Vec<AccountIdent>,
        val: bool,
    },
    SSOLoginFailure {
        name: String,
        error: String,
    },
    PlayerRelogSuccess {
        ident: AccountIdent,
        gs: Box<GameState>,
        session: Box<Session>,
    },
    PlayerRelogDelay {
        ident: AccountIdent,
        session: Box<Session>,
    },
    CopyBattleOrder {
        ident: AccountIdent,
    },
    BackupRes {
        server: ServerID,
        error: Option<String>,
    },
    SaveHoF(ServerID),
    PlayerSetMaxLvl {
        ident: AccountIdent,
        max: u16,
    },
    PlayerSetMaxAttributes {
        ident: AccountIdent,
        max: u32,
    },
    PlayerAttack {
        ident: AccountIdent,
        target: AttackTarget,
    },
    PlayerLure {
        ident: AccountIdent,
        target: LureTarget,
    },
    OpenLink(String),
    SSOSuccess {
        auth_name: String,
        chars: Vec<Session>,
        provider: SSOProvider,
    },
    SSORetry,
    SSOAuthError(String),
    SetMaxThreads(usize),
    SetStartThreads(usize),
    SetBlacklistThr(usize),
    SetAutoFetch(bool),
    SetAutoPoll(bool),
    ViewSubPage {
        player: AccountIdent,
        page: AccountPage,
    },
    SSOImport {
        pos: usize,
    },
    SSOImportAuto {
        ident: SFCharIdent,
    },
    SSOLoginSuccess {
        name: String,
        pass: PWHash,
        chars: Vec<Session>,
        remember: bool,
        auto_login: bool,
    },
    ViewSettings,
    ChangeTheme(AvailableTheme),
    ViewOverview,
    CrawlerRevived {
        server_id: ServerID,
    },
    CrawlerStartup {
        server: ServerID,
        state: Arc<CrawlerState>,
    },
    AutoBattle {
        ident: AccountIdent,
        state: bool,
    },
    AutoLure {
        ident: AccountIdent,
        state: bool,
    },
    PlayerCommandFailed {
        ident: AccountIdent,
        session: Box<Session>,
        attempt: u64,
    },
    PlayerAttackResult {
        ident: AccountIdent,
        session: Box<Session>,
        against: AttackTarget,
        resp: Box<Response>,
    },
    PlayerLureResult {
        ident: AccountIdent,
        session: Box<Session>,
        against: LureTarget,
        resp: Box<Response>,
    },
    AutoBattlePossible {
        ident: AccountIdent,
    },
    OrderChange {
        server: ServerID,
        new: CrawlingOrder,
    },
    Login {
        account: AccountConfig,
        auto_login: bool,
    },
    RememberMe(bool),
    ClearHof(ServerID),
    CrawlerSetThreads {
        server: ServerID,
        new_count: usize,
    },
    PageCrawled,
    RemoveAccount {
        ident: AccountIdent,
    },
    CharacterCrawled {
        server: ServerID,
        que_id: QueID,
        character: CharacterInfo,
    },
    CrawlerDied {
        server: ServerID,
        error: String,
    },
    ShowPlayer {
        ident: AccountIdent,
    },
    CrawlerIdle(ServerID),
    CrawlerNoPlayerResult,
    CrawlerUnable {
        server: ServerID,
        action: CrawlAction,
        error: CrawlerError,
    },
    ViewLogin,
    LoginNameInputChange(String),
    LoginPWInputChange(String),
    LoginServerChange(String),
    LoginSFSubmit,
    LoginRegularSubmit,
    LoginViewChanged(LoginType),
    LoggininSuccess {
        ident: AccountIdent,
        gs: Box<GameState>,
        session: Box<Session>,
        remember: bool,
    },
    LoggininFailure {
        ident: AccountIdent,
        error: String,
    },
    ResetCrawling {
        server: ServerID,
        status: Box<RestoreData>,
    },
    ConfigSetAutoLogin {
        name: String,
        server: ServerID,
        nv: bool,
    },
    ConfigSetAutoBattle {
        name: String,
        server: ServerID,
        nv: bool,
    },
    ConfigSetAutoLure {
        name: String,
        server: ServerID,
        nv: bool,
    },
    UIActive,
    AutoLureIdle,
    AutoLurePossible {
        ident: AccountIdent,
    },
    CopyBestLures {
        ident: AccountIdent,
    },
    SetAction(Option<ActionSelection>),
}

impl Helper {
    pub fn handle_msg(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::UIActive => {}
            Message::PageCrawled => {
                // Gets handled in crawling
            }
            Message::CrawlerDied { server, error } => {
                log::error!("Crawler died on {server} - {error}");

                let Some(server) = self.servers.get_mut(&server) else {
                    return Command::none();
                };
                server.crawling = CrawlingStatus::CrawlingFailed(error)
            }
            Message::CharacterCrawled {
                server,
                que_id,
                character,
            } => {
                let Some(server) = self.servers.get_mut(&server) else {
                    return Command::none();
                };

                trace!("{} crawled {}", server.ident.ident, character.name);

                let CrawlingStatus::Crawling {
                    player_info,
                    equipment,
                    que_id: crawl_que_id,
                    last_update,
                    que,
                    recent_failures,
                    naked,
                    ..
                } = &mut server.crawling
                else {
                    return Command::none();
                };

                let crawler_finished = {
                    let mut lock = que.lock().unwrap();
                    if let Some(pb) = &server.headless_progress {
                        let remaining = lock.count_remaining();
                        let crawled = player_info.len();
                        let total = remaining + crawled;
                        pb.set_length(total as u64);
                        pb.set_position(crawled as u64);
                    };
                    lock.in_flight_accounts.remove(&character.name);
                    lock.todo_pages.is_empty() && lock.todo_accounts.is_empty()
                };

                if *crawl_que_id != que_id {
                    // This was crawled for an outdated que version (we cleared
                    // the que)
                    return Command::none();
                }

                // We were able to make this request, so something must work
                recent_failures.clear();

                *last_update = Local::now();

                handle_new_char_info(character, equipment, player_info, naked);

                if crawler_finished {
                    let mut commands = vec![];
                    let todo: Vec<_> =
                        server.accounts.values().map(|a| a.ident).collect();
                    for acc in todo {
                        commands.push(self.update_best(acc, false));
                    }
                    return Command::batch(commands);
                }

                if let View::Account { ident, .. } = self.current_view {
                    if let Some(current) =
                        server.accounts.get_mut(&ident.account)
                    {
                        let ident = current.ident;
                        return self.update_best(ident, true);
                    }
                }
            }
            Message::CrawlerIdle(server_id) => {
                let Some(server) = self.servers.get_mut(&server_id) else {
                    return Command::none();
                };
                let CrawlingStatus::Crawling {
                    player_info, que, ..
                } = &mut server.crawling
                else {
                    return Command::none();
                };
                let lock = que.lock().unwrap();
                if server.headless_progress.is_none()
                    || !lock.todo_pages.is_empty()
                    || !lock.todo_accounts.is_empty()
                    || player_info.is_empty()
                {
                    return Command::none();
                }
                let backup = lock.create_backup(player_info);
                let ident = server.ident.ident.to_string();
                let id = server.ident.id;

                return Command::perform(
                    async move { backup.write(&ident).await },
                    move |res| Message::BackupRes {
                        server: id,
                        error: res.err().map(|a| a.to_string()),
                    },
                );
            }
            Message::CrawlerNoPlayerResult => {
                // Maybe we want to count this as an error?
                warn!("No player result");
            }
            Message::CrawlerUnable {
                server: server_id,
                action,
                error,
            } => {
                let Some(server) = self.servers.get_mut(&server_id) else {
                    return Command::none();
                };
                let CrawlingStatus::Crawling {
                    que_id,
                    que,
                    recent_failures,
                    crawling_session,
                    ..
                } = &mut server.crawling
                else {
                    return Command::none();
                };

                let mut lock = que.lock().unwrap();
                match &action {
                    CrawlAction::Wait | CrawlAction::InitTodo => {}
                    CrawlAction::Page(a, b) => {
                        if *b != *que_id {
                            return Command::none();
                        }
                        lock.in_flight_pages.retain(|x| x != a);
                        if error == CrawlerError::RateLimit {
                            lock.todo_pages.push(*a);
                            return Command::none();
                        } else {
                            lock.invalid_pages.push(*a);
                        }
                    }
                    CrawlAction::Character(a, b) => {
                        if *b != *que_id {
                            return Command::none();
                        }
                        lock.in_flight_accounts.remove(a);
                        if error == CrawlerError::RateLimit {
                            lock.todo_accounts.push(a.clone());
                            return Command::none();
                        } else {
                            lock.invalid_accounts.push(a.clone());
                        }
                    }
                }

                match error {
                    CrawlerError::NotFound => {
                        return Command::none();
                    }
                    CrawlerError::Generic(err) => warn!(
                        "Crawler was unable to complete: '{action}' on {} -> \
                         {err}",
                        server.ident.id
                    ),
                    CrawlerError::RateLimit => {}
                }

                recent_failures.push(action);

                if recent_failures.len() != 10 {
                    return Command::none();
                }
                debug!("Restarting crawler on {}", server.ident.ident);

                // The last 10 command failed consecutively. This means there
                // is some sort of issue with either the internet connection, or
                // the session. To resolve this, we try to login the crawler
                // again.

                let Some(state) = crawling_session.clone() else {
                    return Command::none();
                };

                let id = server.ident.ident.clone();

                return Command::perform(
                    async move {
                        let mut session_lock = state.session.write().await;
                        loop {
                            debug!("Relog crawler on {}", id);
                            let Ok(resp) = session_lock.login().await else {
                                error!("Could not login crawler on {}", id);
                                sleep(Duration::from_millis(fastrand::u64(
                                    1000..3000,
                                )))
                                .await;
                                continue;
                            };
                            let Ok(new_gs) = GameState::new(resp) else {
                                error!(
                                    "Could not parse GS for crawler on {}",
                                    id
                                );
                                // we can not hold mutex guards accross awaits
                                sleep(Duration::from_millis(fastrand::u64(
                                    1000..3000,
                                )))
                                .await;
                                continue;
                            };
                            sleep(Duration::from_secs(5)).await;

                            let mut gs = state.gs.lock().unwrap();
                            *gs = new_gs;
                            return;
                        }
                    },
                    move |()| Message::CrawlerRevived { server_id },
                );
            }
            Message::ViewLogin => self.current_view = View::Login,
            Message::LoginNameInputChange(a) => self.login_state.name = a,
            Message::LoginSFSubmit => {
                return self.login_sf_acc(
                    self.login_state.name.clone(),
                    PWHash::new(&self.login_state.password),
                    self.login_state.remember_me,
                    false,
                )
            }
            Message::LoginPWInputChange(a) => self.login_state.password = a,
            Message::LoginServerChange(a) => self.login_state.server = a,
            Message::LoginRegularSubmit => {
                let pw_hash = PWHash::new(&self.login_state.password.clone());

                return self.login_regular(
                    self.login_state.name.to_string(),
                    self.login_state.server.to_string(),
                    pw_hash,
                    self.login_state.remember_me,
                    Default::default(),
                );
            }
            Message::LoginViewChanged(a) => {
                self.login_state.login_typ = a;
            }
            Message::LoggininSuccess {
                gs,
                session,
                remember,
                ident,
            } => {
                info!("Successfully logged in {ident}",);

                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(player) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                if remember {
                    match &player.auth {
                        PlayerAuth::Normal(hash) => {
                            self.config.accounts.retain(|a| match &a {
                                AccountConfig::Regular {
                                    name,
                                    server: server_url,
                                    ..
                                } => {
                                    !(name == &player.name
                                        && server_url == &server.ident.url)
                                }
                                _ => true,
                            });
                            self.config.accounts.push(AccountConfig::new(
                                AccountCreds::Regular {
                                    name: player.name.clone(),
                                    pw_hash: hash.clone(),
                                    server: server.ident.url.clone(),
                                },
                            ));
                            _ = self.config.write();
                        }
                        PlayerAuth::SSO => {}
                    }
                }

                let total_players = gs.hall_of_fames.players_total;
                let total_pages = (total_players as usize).div_ceil(PER_PAGE);

                let char_conf =
                    self.config.get_char_conf(&player.name, ident.server_id);

                player.scrapbook_info = ScrapbookInfo::new(&gs, char_conf);
                player.underworld_info = UnderworldInfo::new(&gs, char_conf);

                *player.status.lock().unwrap() =
                    AccountStatus::Idle(session, gs);

                let server_ident = server.ident.ident.clone();
                let server_id = server.ident.id;
                let afn = self.config.auto_fetch_newest;
                match &server.crawling {
                    CrawlingStatus::Waiting => {
                        server.crawling = CrawlingStatus::Restoring;
                        return Command::perform(
                            async move {
                                let backup =
                                    get_newest_backup(server_ident, afn).await;
                                Box::new(
                                    restore_backup(backup, total_pages).await,
                                )
                            },
                            move |backup| Message::ResetCrawling {
                                server: server_id,
                                status: backup,
                            },
                        );
                    }
                    CrawlingStatus::Crawling { .. } => {
                        let ident = player.ident;
                        return self.update_best(ident, false);
                    }
                    _ => (),
                }
            }
            Message::LoggininFailure { error, ident } => {
                error!("Error loggin in {ident}: {error}");
                let Some((_, player)) = self.servers.get_ident(&ident) else {
                    return Command::none();
                };
                *player.status.lock().unwrap() =
                    AccountStatus::FatalError(error)
            }
            Message::ShowPlayer { ident } => {
                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                self.current_view = View::Account {
                    ident,
                    page: AccountPage::Scrapbook,
                };

                let CrawlingStatus::Crawling { last_update, .. } =
                    &server.crawling
                else {
                    return Command::none();
                };

                if account.last_updated < *last_update {
                    let ident = account.ident;
                    return self.update_best(ident, false);
                }
            }
            Message::ResetCrawling {
                server: server_id,
                status,
            } => {
                let Some(server) = self.servers.get_mut(&server_id) else {
                    return Command::none();
                };

                let mut commands = vec![];
                match &mut server.crawling {
                    CrawlingStatus::Waiting | CrawlingStatus::Restoring => {
                        server.crawling = status.into_status();
                        commands.push(server.set_threads(
                            self.config.start_threads, &self.config.base_name,
                        ));
                    }
                    CrawlingStatus::Crawling {
                        que_id,
                        que,
                        player_info,
                        equipment,
                        last_update,
                        recent_failures,
                        naked,
                        threads: _,
                        crawling_session: _,
                    } => {
                        let mut que = que.lock().unwrap();
                        que.que_id = status.que_id;
                        que.todo_accounts = status.todo_accounts;
                        que.todo_pages = status.todo_pages;
                        que.invalid_accounts = status.invalid_accounts;
                        que.invalid_pages = status.invalid_pages;
                        que.order = status.order;
                        que.in_flight_pages = vec![];
                        que.in_flight_accounts = Default::default();
                        *que_id = status.que_id;
                        *naked = status.naked;
                        *player_info = status.player_info;
                        *equipment = status.equipment;
                        *last_update = Local::now();
                        recent_failures.clear();
                        drop(que);
                    }
                    CrawlingStatus::CrawlingFailed(_) => {
                        return Command::none();
                    }
                }

                let CrawlingStatus::Crawling { .. } = &server.crawling else {
                    return Command::none();
                };

                let todo: Vec<_> =
                    server.accounts.values().map(|a| a.ident).collect();
                for acc in todo {
                    commands.push(self.update_best(acc, false));
                }
                return Command::batch(commands);
            }
            Message::RemoveAccount { ident } => {
                let Some(server) = self.servers.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                if let Some(old) = server.accounts.remove(&ident.account) {
                    if matches!(old.auth, PlayerAuth::SSO) {
                        if let Ok(mut sl) = old.status.lock() {
                            if let Some(session) = sl.take_session("Removing") {
                                self.login_state.import_que.push(*session);
                            }
                        }
                    }
                }
                if server.accounts.is_empty() {
                    if let CrawlingStatus::Crawling { threads, .. } =
                        &mut server.crawling
                    {
                        *threads = 0;
                    }
                }

                match &mut self.current_view {
                    View::Account { ident: current, .. }
                        if ident == *current =>
                    {
                        self.current_view = View::Login;
                    }
                    View::Overview { selected, action } => {
                        _ = selected.remove(&ident);
                        *action = None;
                    }
                    _ => {}
                }
            }
            Message::CrawlerSetThreads {
                server: server_id,
                new_count,
            } => {
                let new_count = new_count.clamp(0, self.config.max_threads);
                let Some(server) = self.servers.get_mut(&server_id) else {
                    return Command::none();
                };

                return server.set_threads(new_count, &self.config.base_name);
            }
            Message::ClearHof(server_id) => {
                let Some(server) = self.servers.get_mut(&server_id) else {
                    return Command::none();
                };

                let Some(tp) = server.accounts.iter().find_map(|(_, b)| {
                    match &*b.status.lock().unwrap() {
                        AccountStatus::LoggingInAgain
                        | AccountStatus::LoggingIn
                        | AccountStatus::FatalError(_) => None,
                        AccountStatus::Idle(_, gs)
                        | AccountStatus::Busy(gs, _) => {
                            Some(gs.hall_of_fames.players_total)
                        }
                    }
                }) else {
                    return Command::none();
                };

                let tp = (tp as usize).div_ceil(PER_PAGE);

                let id = server.ident.id;

                return Command::perform(
                    async move { Box::new(restore_backup(None, tp).await) },
                    move |res| Message::ResetCrawling {
                        server: id,
                        status: res,
                    },
                );
            }
            Message::RememberMe(val) => self.login_state.remember_me = val,
            Message::Login {
                account,
                auto_login,
            } => match account {
                AccountConfig::Regular {
                    name,
                    pw_hash,
                    server,
                    ..
                } => {
                    return self.login_regular(
                        name, server, pw_hash, false, auto_login,
                    );
                }
                AccountConfig::SF { name, pw_hash, .. } => {
                    return self.login_sf_acc(name, pw_hash, false, auto_login);
                }
            },
            Message::OrderChange { server, new } => {
                let Some(server) = self.servers.get_mut(&server) else {
                    return Command::none();
                };
                if let CrawlingStatus::Crawling { que, .. } = &server.crawling {
                    let mut que = que.lock().unwrap();
                    que.order = new;
                    new.apply_order(&mut que.todo_pages);
                }
            }
            Message::AutoBattlePossible { ident } => {
                let refetch = self.update_best(ident, true);

                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                let CrawlingStatus::Crawling { .. } = &server.crawling else {
                    return Command::none();
                };

                let mut status = account.status.lock().unwrap();
                let AccountStatus::Idle(_, gs) = &*status else {
                    return refetch;
                };
                let next = gs.arena.next_free_fight.unwrap_or_default();
                if next > Local::now() + Duration::from_millis(200) {
                    return refetch;
                }

                let Some(mut session) = status.take_session("A Fighting")
                else {
                    return refetch;
                };

                let Some(si) = &account.scrapbook_info else {
                    status.put_session(session);
                    return refetch;
                };

                let total_len = si.best.len();
                let new_len = si.best.iter().filter(|a| !a.is_old()).count();

                // The list will be mostly old at startup.
                // Therefore, we should wait until the list is mostly fetched,
                // until we actually start. This is not new_len == total_len in
                // case there is an off by one error/other bug somewhere, that
                // would leave the auto-battle perma stuck here
                if total_len == 0 || (new_len as f32 / total_len as f32) < 0.9 {
                    status.put_session(session);
                    return refetch;
                }

                let Some(target) =
                    si.best.iter().find(|a| !a.is_old()).cloned()
                else {
                    status.put_session(session);
                    return refetch;
                };
                drop(status);

                let tn = target.info.name.clone();
                let fight = Command::perform(
                    async move {
                        let cmd = sf_api::command::Command::Fight {
                            name: tn,
                            use_mushroom: false,
                        };
                        let resp = session.send_command(&cmd).await;
                        (resp, session)
                    },
                    move |r| match r.0 {
                        Ok(resp) => Message::PlayerAttackResult {
                            ident,
                            session: r.1,
                            against: target,
                            resp: Box::new(resp),
                        },
                        Err(_) => Message::PlayerCommandFailed {
                            ident,
                            session: r.1,
                            attempt: 0,
                        },
                    },
                );

                return Command::batch([refetch, fight]);
            }
            Message::PlayerCommandFailed {
                ident,
                mut session,
                attempt,
            } => {
                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(player) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                let mut lock = player.status.lock().unwrap();
                *lock = AccountStatus::LoggingInAgain;
                drop(lock);
                warn!("Logging in {ident} again");
                return Command::perform(
                    async move {
                        let Ok(resp) = session.login().await else {
                            sleep(Duration::from_secs(5)).await;
                            return Err(session);
                        };
                        let Ok(gamestate) = GameState::new(resp) else {
                            sleep(Duration::from_secs(5)).await;
                            return Err(session);
                        };
                        sleep(Duration::from_secs(attempt)).await;
                        Ok((Box::new(gamestate), session))
                    },
                    move |res| match res {
                        Ok((gs, session)) => {
                            Message::PlayerRelogSuccess { ident, gs, session }
                        }
                        Err(session) => Message::PlayerCommandFailed {
                            ident,
                            session,
                            attempt: attempt + 1,
                        },
                    },
                );
            }
            Message::PlayerAttackResult {
                ident,
                session,
                against,
                resp,
            } => {
                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                let v = account.status.clone();
                let mut lock = v.lock().unwrap();

                let AccountStatus::Busy(s, _) = &mut *lock else {
                    return Command::none();
                };

                if let Err(e) = s.update(*resp) {
                    // it would *probably* be ok to just ignore this in most
                    // cases, but whatever
                    *lock = AccountStatus::FatalError(e.to_string());
                    return Command::none();
                };

                let Some(last) = &s.last_fight else {
                    return Command::none();
                };

                let nt = against.info.name.clone();
                let ut = against.info.uid;

                let Some(si) = &mut account.scrapbook_info else {
                    return Command::none();
                };

                if last.has_player_won {
                    for new in &against.info.equipment {
                        si.scrapbook.items.insert(*new);
                    }
                }

                si.attack_log.push((
                    Local::now(),
                    against,
                    last.has_player_won,
                ));

                let mut res = Command::none();

                if !last.has_player_won {
                    si.blacklist.entry(ut).or_insert((nt, 0)).1 += 1;
                } else if let CrawlingStatus::Crawling { .. } = &server.crawling
                {
                    let ident = account.ident;
                    res = self.update_best(ident, false);
                }

                lock.put_session(session);
                return res;
            }
            Message::AutoBattle { ident, state } => {
                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(player) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                let Some(si) = &mut player.scrapbook_info else {
                    return Command::none();
                };

                si.auto_battle = state;
            }
            Message::CrawlerStartup { server, state } => {
                let Some(server) = self.servers.get_mut(&server) else {
                    return Command::none();
                };

                let CrawlingStatus::Crawling {
                    crawling_session, ..
                } = &mut server.crawling
                else {
                    return Command::none();
                };
                *crawling_session = Some(state);
            }
            Message::CrawlerRevived { server_id } => {
                info!("Crawler revived");
                let Some(server) = self.servers.get_mut(&server_id) else {
                    return Command::none();
                };
                let CrawlingStatus::Crawling {
                    que,
                    recent_failures,
                    ..
                } = &mut server.crawling
                else {
                    return Command::none();
                };

                let mut que = que.lock().unwrap();

                let mut ok_pages = vec![];
                let mut ok_character = vec![];
                for action in recent_failures.drain(..) {
                    match action {
                        CrawlAction::Wait | CrawlAction::InitTodo => {}
                        CrawlAction::Page(page, que_id) => {
                            if que_id != que.que_id {
                                continue;
                            }
                            ok_pages.push(page);
                        }
                        CrawlAction::Character(name, que_id) => {
                            if que_id != que.que_id {
                                continue;
                            }
                            ok_character.push(name);
                        }
                    }
                }

                que.invalid_pages.retain(|a| !ok_pages.contains(a));
                que.invalid_accounts.retain(|a| !ok_character.contains(a));
                que.todo_accounts.append(&mut ok_character);
                que.todo_pages.append(&mut ok_pages);
            }
            Message::ViewOverview => {
                self.current_view = View::Overview {
                    selected: Default::default(),
                    action: Default::default(),
                };
            }
            Message::ChangeTheme(theme) => {
                self.config.theme = theme;
                _ = self.config.write();
            }
            Message::ViewSettings => {
                self.current_view = View::Settings;
            }
            Message::SSOLoginSuccess {
                name,
                pass,
                mut chars,
                remember,
                auto_login,
            } => {
                let ident = SSOIdent::SF(name.clone());

                let Some(res) = self
                    .login_state
                    .active_sso
                    .iter_mut()
                    .find(|a| a.ident == ident)
                else {
                    // Already logged in
                    return Command::none();
                };
                if remember {
                    self.config.accounts.retain(|a| match &a {
                        AccountConfig::Regular { .. } => true,
                        AccountConfig::SF { name: uuu, .. } => {
                            name.to_lowercase() != uuu.to_lowercase()
                        }
                    });

                    self.config.accounts.push(AccountConfig::SF {
                        name: name.clone(),
                        pw_hash: pass,
                        characters: chars
                            .iter()
                            .map(|a| SFAccCharacter {
                                config: CharacterConfig::default(),
                                ident: SFCharIdent {
                                    name: a.username().to_string(),
                                    server: a.server_url().as_str().to_string(),
                                },
                            })
                            .collect(),
                    });
                    _ = self.config.write();
                }

                if let Some(existing) = self.config.get_sso_accounts_mut(&name)
                {
                    let mut new: HashSet<(ServerIdent, String)> =
                        HashSet::new();
                    for char in &chars {
                        let name = char.username().trim().to_lowercase();
                        new.insert((
                            ServerIdent::new(char.server_url().as_str()),
                            name,
                        ));
                    }

                    let mut modified = false;

                    existing.retain(|a| {
                        let res = new.remove(&(
                            ServerIdent::new(&a.ident.server),
                            a.ident.name.trim().to_lowercase(),
                        ));
                        if !res {
                            modified = true;
                            info!("Removed a SSO char")
                        }
                        res
                    });

                    for (server, name) in new {
                        modified = true;
                        info!("Registered a a new SSO chars");
                        existing.push(SFAccCharacter {
                            config: CharacterConfig::default(),
                            ident: SFCharIdent {
                                name,
                                server: server.url.to_string(),
                            },
                        })
                    }

                    if modified {
                        _ = self.config.write();
                    }
                }

                self.login_state.import_que.append(&mut chars);

                res.status = SSOLoginStatus::Success;
                if auto_login {
                    for acc in &self.config.accounts {
                        let AccountConfig::SF {
                            name: s_name,
                            characters,
                            ..
                        } = acc
                        else {
                            continue;
                        };
                        if s_name != &name {
                            continue;
                        }
                        let mut commands = vec![];
                        for SFAccCharacter { ident, config } in characters {
                            if !config.login {
                                continue;
                            }
                            let ident = ident.clone();
                            commands
                                .push(Command::perform(async {}, move |_| {
                                    Message::SSOImportAuto { ident }
                                }))
                        }
                        return Command::batch(commands);
                    }
                }

                if self.current_view == View::Login
                    && self.login_state.login_typ == LoginType::SSOAccounts
                {
                    self.login_state.login_typ = LoginType::SSOChars;
                };
            }
            Message::SSOImport { pos } => {
                // TODO: Bounds check this?
                let account = self.login_state.import_que.remove(pos);
                return self.login(account, false, PlayerAuth::SSO, false);
            }
            Message::ViewSubPage { player, page } => {
                self.current_view = View::Account {
                    ident: player,
                    page,
                }
            }
            Message::SetAutoFetch(b) => {
                self.config.auto_fetch_newest = b;
                _ = self.config.write();
            }
            Message::SetMaxThreads(nv) => {
                self.config.max_threads = nv.clamp(0, 50);
                self.config.start_threads = self
                    .config
                    .start_threads
                    .clamp(0, 50.min(self.config.max_threads));
                _ = self.config.write();
            }
            Message::SetStartThreads(nv) => {
                self.config.start_threads =
                    nv.clamp(0, 50.min(self.config.max_threads));
                _ = self.config.write();
            }
            Message::SSOSuccess {
                auth_name,
                mut chars,
                provider,
            } => {
                let ident = match provider {
                    SSOProvider::Google => SSOIdent::Google(auth_name.clone()),
                    SSOProvider::Steam => SSOIdent::Steam(auth_name.clone()),
                };
                if self.login_state.active_sso.iter().any(|a| a.ident == ident)
                {
                    // Already logged in
                    return Command::none();
                };

                let new_sso = SSOLogin {
                    sso_id: fastrand::u64(..),
                    ident,
                    status: SSOLoginStatus::Success,
                };

                self.login_state.active_sso.push(new_sso);
                self.login_state.import_que.append(&mut chars);

                if self.current_view == View::Login
                    && self.login_state.login_typ == LoginType::Google
                    || self.login_state.login_typ == LoginType::Steam
                {
                    self.login_state.login_typ = LoginType::SSOChars;
                };
            }
            Message::SSORetry => {
                // The subscription will handle this
            }
            Message::SSOAuthError(_) => {
                // TODO: Display this I guess
            }
            Message::OpenLink(url) => {
                _ = open::that(url);
            }
            Message::PlayerAttack { ident, target } => {
                let Some(server) = self.servers.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };
                let CrawlingStatus::Crawling { .. } = &server.crawling else {
                    return Command::none();
                };

                let mut status = account.status.lock().unwrap();
                let AccountStatus::Idle(_, gs) = &*status else {
                    return Command::none();
                };
                let next = gs.arena.next_free_fight.unwrap_or_default();
                if next > Local::now() + Duration::from_millis(200)
                    && gs.character.mushrooms == 0
                {
                    return Command::none();
                }

                let Some(mut session) = status.take_session("Fighting") else {
                    return Command::none();
                };
                drop(status);
                let ident = account.ident;
                let tn = target.info.name.clone();
                return Command::perform(
                    async move {
                        let cmd = sf_api::command::Command::Fight {
                            name: tn,
                            use_mushroom: false,
                        };
                        let resp = session.send_command(&cmd).await;
                        (resp, session)
                    },
                    move |r| match r.0 {
                        Ok(resp) => Message::PlayerAttackResult {
                            ident,
                            session: r.1,
                            against: target,
                            resp: Box::new(resp),
                        },
                        Err(_) => Message::PlayerCommandFailed {
                            ident,
                            session: r.1,
                            attempt: 0,
                        },
                    },
                );
            }
            Message::PlayerSetMaxLvl { ident, max } => {
                let Some(server) = self.servers.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };
                let Some(si) = &mut account.scrapbook_info else {
                    return Command::none();
                };
                si.max_level = max;
                return self.update_best(ident, false);
            }
            Message::PlayerSetMaxAttributes { ident, max } => {
                let Some(server) = self.servers.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };
                let Some(si) = &mut account.scrapbook_info else {
                    return Command::none();
                };
                si.max_attributes = max;
                return self.update_best(ident, false);
            }
            Message::SaveHoF(server_id) => {
                let Some(server) = self.servers.get(&server_id) else {
                    return Command::none();
                };

                let CrawlingStatus::Crawling {
                    que, player_info, ..
                } = &server.crawling
                else {
                    return Command::none();
                };

                let lock = que.lock().unwrap();
                let backup = lock.create_backup(player_info);
                drop(lock);
                let id = server.ident.id;
                let ident = server.ident.ident.to_string();

                return Command::perform(
                    async move { backup.write(&ident).await },
                    move |res| Message::BackupRes {
                        server: id,
                        error: res.err().map(|a| a.to_string()),
                    },
                );
            }
            Message::BackupRes {
                server: server_id,
                error,
            } => {
                // TODO: Display error?
                let Some(server) = self.servers.get_mut(&server_id) else {
                    return Command::none();
                };
                let Some(pb) = server.headless_progress.clone() else {
                    return Command::none();
                };
                if let Some(err) = error {
                    pb.println(err)
                }
                self.servers.0.remove(&server_id);
                pb.finish_and_clear();
                return Command::perform(async {}, |_| {
                    Message::NextCLICrawling
                });
            }
            Message::CopyBattleOrder { ident } => {
                let Some((server, account)) = self.servers.get_ident(&ident)
                else {
                    return Command::none();
                };

                let CrawlingStatus::Crawling {
                    player_info,
                    equipment,
                    que,
                    ..
                } = &server.crawling
                else {
                    return Command::none();
                };

                let Some(si) = &account.scrapbook_info else {
                    return Command::none();
                };

                let mut best = si.best.first().cloned();
                let mut scrapbook = si.scrapbook.items.clone();

                let mut per_player_counts = calc_per_player_count(
                    player_info, equipment, &scrapbook, si,
                    self.config.blacklist_threshold,
                );

                let mut target_list = Vec::new();
                let mut loop_count = 0;
                let lock = que.lock().unwrap();
                let invalid =
                    lock.invalid_accounts.iter().map(|a| a.as_str()).collect();

                while let Some(AttackTarget { missing, info }) = best {
                    if loop_count > 300 || missing == 0 {
                        break;
                    }
                    loop_count += 1;

                    for eq in &info.equipment {
                        if scrapbook.contains(eq) {
                            continue;
                        }
                        let Some(players) = equipment.get(eq) else {
                            continue;
                        };
                        // We decrease the new equipment count of all players,
                        // that have the same item as
                        // the one we just "found"
                        for player in players {
                            let ppc =
                                per_player_counts.entry(*player).or_insert(1);
                            *ppc = ppc.saturating_sub(1);
                        }
                    }

                    scrapbook.extend(info.equipment);
                    target_list.push(info.name);
                    let best_players =
                        find_best(&per_player_counts, player_info, 1, &invalid);
                    best = best_players.into_iter().next();
                }
                drop(lock);
                return iced::clipboard::write(target_list.join("/"));
            }
            Message::PlayerRelogSuccess { ident, gs, session } => {
                info!("Relogin success");
                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(player) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                let mut lock = player.status.lock().unwrap();
                *lock = AccountStatus::Busy(gs, "Waiting".into());
                drop(lock);
                // For some reason the game does not like sending requests
                // immediately
                return Command::perform(
                    async {
                        sleep(Duration::from_secs(10)).await;
                    },
                    move |_| Message::PlayerRelogDelay { ident, session },
                );
            }
            Message::SSOLoginFailure { name, error } => {
                self
                    .login_state
                    .active_sso
                    .retain(|a| !matches!(&a.ident, SSOIdent::SF(s) if s.as_str() == name.as_str()));
                self.login_state.login_typ = LoginType::SFAccount;
                self.login_state.error = Some(error)
            }
            Message::PlayerLure { ident, target } => {
                let Some(server) = self.servers.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };
                let Some(ud) = &account.underworld_info else {
                    return Command::none();
                };
                if ud.underworld.lured_today >= 5 {
                    return Command::none();
                }

                let CrawlingStatus::Crawling { .. } = &server.crawling else {
                    return Command::none();
                };

                let mut status = account.status.lock().unwrap();
                let Some(mut session) = status.take_session("Luring") else {
                    return Command::none();
                };
                drop(status);
                let ident = account.ident;
                let tid = target.uid;
                return Command::perform(
                    async move {
                        let cmd = sf_api::command::Command::UnderworldAttack {
                            player_id: tid,
                        };
                        let resp = session.send_command(&cmd).await;
                        (resp, session)
                    },
                    move |r| match r.0 {
                        Ok(resp) => Message::PlayerLureResult {
                            ident,
                            session: r.1,
                            against: target,
                            resp: Box::new(resp),
                        },
                        Err(_) => Message::PlayerCommandFailed {
                            ident,
                            session: r.1,
                            attempt: 0,
                        },
                    },
                );
            }
            Message::PlayerLureResult {
                ident,
                session,
                against,
                resp,
            } => {
                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                let v = account.status.clone();
                let mut lock = v.lock().unwrap();

                let AccountStatus::Busy(s, _) = &mut *lock else {
                    return Command::none();
                };

                if let Err(e) = s.update(*resp) {
                    // it would *probably* be ok to just ignore this in most
                    // cases, but whatever
                    *lock = AccountStatus::FatalError(e.to_string());
                    return Command::none();
                };

                let Some(last) = &s.last_fight else {
                    return Command::none();
                };

                let Some(si) = &mut account.underworld_info else {
                    return Command::none();
                };

                si.attack_log.push((
                    Local::now(),
                    against.name,
                    last.has_player_won,
                ));

                if let Some(underworld) = s.underworld.as_ref() {
                    si.underworld = underworld.clone();
                }
                lock.put_session(session);
            }
            Message::PlayerNotPolled { ident } => {
                warn!("Unable to update {ident}")
            }
            Message::PlayerPolled { ident } => {
                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };
                let mut lock = account.status.lock().unwrap();
                let gs = match &mut *lock {
                    AccountStatus::Busy(gs, _) | AccountStatus::Idle(_, gs) => {
                        gs
                    }
                    _ => {
                        return Command::none();
                    }
                };

                if let Some(sbi) = &mut account.underworld_info {
                    if let Some(sb) = &gs.underworld {
                        sbi.underworld = sb.clone();
                    }
                }

                drop(lock);
            }
            Message::PlayerSetMaxUndergroundLvl { ident, lvl } => {
                let Some(server) = self.servers.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };
                let Some(si) = &mut account.underworld_info else {
                    return Command::none();
                };
                si.max_level = lvl;
                return self.update_best(ident, false);
            }
            Message::UpdateResult(should_update) => {
                self.should_update = should_update;
            }
            Message::SetAutoPoll(new_val) => {
                self.config.auto_poll = new_val;
                _ = self.config.write();
            }
            Message::AdvancedLevelRestrict(val) => {
                self.config.show_crawling_restrict = val;
                _ = self.config.write();
            }
            Message::CrawlerSetMinMax { server, min, max } => {
                let Some(server) = self.servers.get_mut(&server) else {
                    return Command::none();
                };
                if let CrawlingStatus::Crawling { que, .. } = &server.crawling {
                    let mut que = que.lock().unwrap();
                    que.min_level = min.max(1);
                    que.max_level = max.max(min).min(9999);

                    debug!(
                        "Changed MinMax to {}/{}",
                        que.min_level, que.max_level
                    );
                    let mut to_remove = IntSet::default();
                    for (lvl, _) in
                        que.lvl_skipped_accounts.range(0..que.min_level)
                    {
                        to_remove.insert(*lvl);
                    }
                    for (lvl, _) in
                        que.lvl_skipped_accounts.range(que.max_level + 1..)
                    {
                        to_remove.insert(*lvl);
                    }
                    for lvl in to_remove {
                        let Some(mut todo) =
                            que.lvl_skipped_accounts.remove(&lvl)
                        else {
                            continue;
                        };
                        que.todo_accounts.append(&mut todo);
                    }
                }
            }
            Message::ShowClasses(val) => {
                self.config.show_class_icons = val;
                _ = self.config.write();
            }
            Message::NextCLICrawling => {
                let Some(cli) = &mut self.cli_crawling else {
                    return Command::none();
                };
                let pb = cli.mbp.add(ProgressBar::new_spinner());

                let Some(url) = cli.todo_servers.pop() else {
                    cli.active -= 1;
                    if cli.active == 0 {
                        pb.println("Finished Crawling all servers");
                        pb.finish_and_clear();
                        std::process::exit(0);
                    }
                    pb.finish_and_clear();
                    return Command::none();
                };
                let threads = cli.threads;
                return match self.force_init_crawling(&url, threads, pb.clone())
                {
                    Some(s) => s,
                    None => {
                        pb.println(format!(
                            "Could not init crawling on: {url}"
                        ));
                        pb.finish_and_clear();
                        return Command::perform(async {}, |_| {
                            Message::NextCLICrawling
                        });
                    }
                };
            }
            Message::CrawlAllRes {
                servers,
                concurrency,
            } => {
                let Some(cli) = &mut self.cli_crawling else {
                    return Command::none();
                };
                let Some(servers) = servers else {
                    _ = cli.mbp.println("Could not fetch server list");
                    std::process::exit(1);
                };
                cli.todo_servers = servers;
                let mut res = vec![];
                for _ in 0..concurrency {
                    res.push(Command::perform(async {}, |_| {
                        Message::NextCLICrawling
                    }))
                }
                return Command::batch(res);
            }
            Message::FontLoaded(_) => {}
            Message::PlayerRelogDelay { ident, session } => {
                let Some(server) = self.servers.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                let mut lock = account.status.lock().unwrap();
                lock.put_session(session);
                drop(lock);
            }
            Message::SSOImportAuto { ident } => {
                let i_name = ident.name.to_lowercase();
                let i_server = ServerIdent::new(&ident.server);

                let pos = self.login_state.import_que.iter().position(|char| {
                    let server = ServerIdent::new(char.server_url().as_str());
                    let name = char.username().to_lowercase();
                    server == i_server && name == i_name
                });
                let Some(pos) = pos else {
                    return Command::none();
                };
                let account = self.login_state.import_que.remove(pos);
                return self.login(account, false, PlayerAuth::SSO, true);
            }
            Message::SetOverviewSelected { ident, val } => {
                let View::Overview { selected, action } =
                    &mut self.current_view
                else {
                    return Command::none();
                };
                *action = None;
                if val {
                    for v in ident {
                        selected.insert(v);
                    }
                } else {
                    for v in ident {
                        selected.remove(&v);
                    }
                }
            }
            Message::ConfigSetAutoLogin { name, server, nv } => {
                let Some(config) = self.config.get_char_conf_mut(&name, server)
                else {
                    return Command::none();
                };
                config.login = nv;
                _ = self.config.write();
            }
            Message::ConfigSetAutoBattle { name, server, nv } => {
                let Some(config) = self.config.get_char_conf_mut(&name, server)
                else {
                    return Command::none();
                };
                config.auto_battle = nv;
                _ = self.config.write();
            }
            Message::SetBlacklistThr(nv) => {
                self.config.blacklist_threshold = nv.max(1);
                _ = self.config.write();
            }
            Message::AutoLureIdle => {}
            Message::AutoLurePossible { ident } => {
                let refetch = self.update_best(ident, true);

                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(account) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                let CrawlingStatus::Crawling { .. } = &server.crawling else {
                    return Command::none();
                };

                let mut status = account.status.lock().unwrap();
                let AccountStatus::Idle(_, gs) = &*status else {
                    return refetch;
                };

                let Some(0..=4) = gs.underworld.as_ref().map(|a| a.lured_today)
                else {
                    return refetch;
                };

                let Some(mut session) = status.take_session("Luring") else {
                    return refetch;
                };

                let Some(ui) = &account.underworld_info else {
                    status.put_session(session);
                    return refetch;
                };

                let total_len = ui.best.len();
                let new_len = ui.best.iter().filter(|a| !a.is_old()).count();

                // Thelist will be mostly old at startup.
                // Therefore, we should wait until the list is mostly fetched,
                // until we actually start. This is not new_len == total_len in
                // case there is an off by one error/other bug somewhere, that
                // would leave the auto-battle perma stuck here
                if total_len == 0 || (new_len as f32 / total_len as f32) < 0.9 {
                    status.put_session(session);
                    return refetch;
                }

                let Some(target) =
                    ui.best.iter().find(|a| !a.is_old()).cloned()
                else {
                    status.put_session(session);
                    return refetch;
                };
                drop(status);
                info!("Auto Underworld attack {ident}");
                let fight = Command::perform(
                    async move {
                        let cmd = sf_api::command::Command::UnderworldAttack {
                            player_id: target.uid,
                        };
                        let resp = session.send_command(&cmd).await;
                        (resp, session)
                    },
                    move |r| match r.0 {
                        Ok(resp) => Message::PlayerLureResult {
                            ident,
                            session: r.1,
                            against: LureTarget {
                                uid: target.uid,
                                name: target.name,
                            },
                            resp: Box::new(resp),
                        },
                        Err(_) => Message::PlayerCommandFailed {
                            ident,
                            session: r.1,
                            attempt: 0,
                        },
                    },
                );

                return Command::batch([refetch, fight]);
            }
            Message::ConfigSetAutoLure { name, server, nv } => {
                let Some(config) = self.config.get_char_conf_mut(&name, server)
                else {
                    return Command::none();
                };
                config.auto_lure = nv;
                _ = self.config.write();
            }
            Message::AutoLure { ident, state } => {
                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(player) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                let Some(si) = &mut player.underworld_info else {
                    return Command::none();
                };

                si.auto_lure = state;
            }
            Message::CopyBestLures { ident } => {
                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(player) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                let Some(si) = &mut player.underworld_info else {
                    return Command::none();
                };

                let mut res = format!(
                    "Best lure targets on {}. Max Lvl = {}\n",
                    server.ident.url, si.max_level
                );

                for a in &si.best {
                    if a.is_old() {
                        continue;
                    }
                    _ = res.write_fmt(format_args!(
                        "lvl: {:3}, items: {}, name: {}\n",
                        a.level,
                        a.equipment.len(),
                        a.name,
                    ));
                }

                return iced::clipboard::write(res);
            }
            Message::SetAction(a) => {
                let View::Overview { action, .. } = &mut self.current_view
                else {
                    return Command::none();
                };
                *action = a;
            }
            Message::MultiAction { action } => {
                let View::Overview {
                    action: ac,
                    selected,
                } = &mut self.current_view
                else {
                    return Command::none();
                };
                let targets = match ac {
                    Some(ActionSelection::Multi) => {
                        selected.iter().copied().collect()
                    }
                    Some(ActionSelection::Character(c)) => vec![*c],
                    None => return Command::none(),
                };

                *ac = None;

                let messages = targets
                    .into_iter()
                    .map(|a| match action {
                        OverviewAction::Logout => {
                            Message::RemoveAccount { ident: a }
                        }
                        OverviewAction::AutoBattle(nv) => Message::AutoBattle {
                            ident: a,
                            state: nv,
                        },
                    })
                    .map(|a| Command::perform(async {}, move |_| a));

                return Command::batch(messages);
            }
        }
        Command::none()
    }
}
