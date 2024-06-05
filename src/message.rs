use std::{sync::Arc, time::Duration};

use chrono::Local;
use iced::Command;
use log::{error, trace, warn};
use sf_api::{
    gamestate::GameState,
    session::{PWHash, Response, Session},
    sso::SSOProvider,
};
use tokio::time::sleep;

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
    SetAutoFetch(bool),
    SetAutoPoll(bool),
    ViewSubPage {
        player: AccountIdent,
        page: AccountPage,
    },
    SSOImport {
        pos: usize,
    },
    SSOLoginSuccess {
        name: String,
        pass: PWHash,
        chars: Vec<Session>,
        remember: bool,
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
    AutoFightPossible {
        ident: AccountIdent,
    },
    OrderChange {
        server: ServerID,
        new: CrawlingOrder,
    },
    LoginRegular {
        name: String,
        pwhash: PWHash,
        server: String,
    },
    LoginSF {
        name: String,
        pwhash: PWHash,
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
}

impl Helper {
    pub fn handle_msg(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::PageCrawled => {
                // Gets handled in crawling
            }
            Message::CrawlerDied { server, error } => {
                log::error!("Crawler died on {server:?} - {error}");

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

                    lock.in_flight_accounts.retain(|a| a != &character.name);
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
            } => {
                let Some(server) = self.servers.get_mut(&server_id) else {
                    return Command::none();
                };
                warn!(
                    "Crawler was unable to do: {action:?} on {}",
                    server.ident.ident
                );
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
                        if *b == *que_id {
                            lock.invalid_pages.push(*a);
                            lock.in_flight_pages.retain(|x| x != a);
                        }
                    }
                    CrawlAction::Character(a, b) => {
                        if *b == *que_id {
                            lock.invalid_accounts.push(a.to_string());
                            lock.in_flight_accounts.retain(|x| x != a);
                        }
                    }
                }
                drop(lock);

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
                info!(
                    "Successfully logged in {ident:?} on {}",
                    session.server_url()
                );

                let Some(server) = self.servers.0.get_mut(&ident.server_id)
                else {
                    return Command::none();
                };
                let Some(player) = server.accounts.get_mut(&ident.account)
                else {
                    return Command::none();
                };

                player.scrapbook_info = ScrapbookInfo::new(&gs, &self.config);

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

                player.scrapbook_info = ScrapbookInfo::new(&gs, &self.config);
                player.underworld_info = UnderworldInfo::new(&gs);

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
                error!("Error loggin in {ident:?}: {error}");
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

                match &mut server.crawling {
                    CrawlingStatus::Waiting | CrawlingStatus::Restoring => {
                        server.crawling = status.into_status();
                    }
                    CrawlingStatus::Crawling {
                        que_id,
                        que,
                        player_info,
                        equipment,
                        last_update,
                        recent_failures,
                        ..
                    } => {
                        let mut que = que.lock().unwrap();
                        que.que_id = status.que_id;
                        que.todo_accounts = status.todo_accounts;
                        que.todo_pages = status.todo_pages;
                        que.invalid_accounts = status.invalid_accounts;
                        que.invalid_pages = status.invalid_pages;
                        que.order = status.order;
                        que.in_flight_pages = vec![];
                        que.in_flight_accounts = vec![];
                        *que_id = status.que_id;
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

                let mut commands = vec![];
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
                            if let Some(session) = sl.take_session() {
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
                let View::Account { ident: current, .. } = self.current_view
                else {
                    return Command::none();
                };
                if ident == current {
                    self.current_view = View::Login;
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
                        | AccountStatus::Busy(gs) => {
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
            Message::LoginRegular {
                name,
                pwhash,
                server,
            } => {
                return self.login_regular(name, server, pwhash, false);
            }
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
            Message::AutoFightPossible { ident } => {
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

                let Some(mut session) = status.take_session() else {
                    return refetch;
                };
                drop(status);

                let Some(si) = &account.scrapbook_info else {
                    return Command::none();
                };

                let Some(target) = si.best.first().cloned() else {
                    let mut status = account.status.lock().unwrap();
                    status.put_session(session);
                    return refetch;
                };

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
                warn!("Logging in {ident:?} again");
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

                let AccountStatus::Busy(s) = &mut *lock else {
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
                println!("Crawler revived");
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
                self.current_view = View::Overview;
            }
            Message::ChangeTheme(theme) => {
                self.config.theme = theme;
                _ = self.config.write();
            }
            Message::ViewSettings => {
                self.current_view = View::Settings;
            }
            Message::LoginSF { name, pwhash } => {
                return self.login_sf_acc(name, pwhash, false);
            }
            Message::SSOLoginSuccess {
                name,
                pass,
                mut chars,
                remember,
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
                        AccountConfig::SF { name: uuu, .. } => &name != uuu,
                    });

                    self.config.accounts.push(AccountConfig::SF {
                        name,
                        pw_hash: pass,
                        characters: Default::default(),
                    });
                    _ = self.config.write();
                }

                self.login_state.import_que.append(&mut chars);
                res.status = SSOLoginStatus::Success;
                if self.current_view == View::Login
                    && self.login_state.login_typ == LoginType::SSOAccounts
                {
                    self.login_state.login_typ = LoginType::SSOChars;
                };
            }
            Message::SSOImport { pos } => {
                // TODO: Bounds check this?
                let account = self.login_state.import_que.remove(pos);
                return self.login(account, false, PlayerAuth::SSO);
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

                let Some(mut session) = status.take_session() else {
                    return Command::none();
                };
                drop(status);
                let ident = account.ident;
                let refetch_cmd = self.update_best(ident, true);
                let tn = target.info.name.clone();
                let attack = Command::perform(
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

                return Command::batch([refetch_cmd, attack]);
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
                );

                let mut target_list = Vec::new();
                let mut loop_count = 0;
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
                        find_best(&per_player_counts, player_info, 1);
                    best = best_players.into_iter().next();
                }

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
                *lock = AccountStatus::Busy(gs);
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
                let Some(mut session) = status.take_session() else {
                    return Command::none();
                };
                drop(status);
                let ident = account.ident;
                let refetch_cmd = self.update_best(ident, true);
                let tid = target.uid;
                let attack = Command::perform(
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

                return Command::batch([refetch_cmd, attack]);
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

                let AccountStatus::Busy(s) = &mut *lock else {
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
                warn!("Unable to poll {ident:?}")
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
                    AccountStatus::Busy(gs) | AccountStatus::Idle(_, gs) => gs,
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
        }
        Command::none()
    }
}
