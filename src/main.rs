#![windows_subsystem = "windows"]
mod backup;
mod config;
mod crawler;
mod login;
mod message;
mod player;
mod server;
mod ui;

use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    sync::{atomic::AtomicU64, Arc, Mutex},
    time::Duration,
};

use chrono::{Local, NaiveDate, Utc};
use config::{CharacterConfig, Config};
use crawler::{CrawlAction, Crawler, CrawlerState, CrawlingOrder, WorkerQue};
use iced::{
    executor, subscription, theme,
    widget::{button, container, horizontal_space, row, text},
    Alignment, Application, Command, Element, Length, Settings, Subscription,
    Theme,
};
use log::{debug, info, trace};
use log4rs::{
    append::{
        console::{ConsoleAppender, Target},
        file::FileAppender,
    },
    config::{Appender, Logger, Root},
    encode::pattern::PatternEncoder,
};
use login::{Auth, LoginState, LoginType, SSOStatus, SSOValidator};
use nohash_hasher::IntMap;
use player::{AccountInfo, AccountStatus, AutoAttackChecker, AutoPoll};
use serde::{Deserialize, Serialize};
use server::{CrawlingStatus, ServerIdent, ServerInfo, Servers};
use sf_api::{gamestate::unlockables::EquipmentIdent, sso::SSOProvider};

use crate::{
    config::{AccountCreds, AvailableTheme},
    message::Message,
};

pub const PER_PAGE: usize = 51;

fn main() -> iced::Result {
    let config = get_log_config();
    log4rs::init_config(config).unwrap();
    info!("Starting up");

    let mut settings = Settings::default();
    settings.window.min_size = Some(iced::Size {
        width: 700.0,
        height: 400.0,
    });

    let raw_img = include_bytes!("../assets/icon.ico");
    let img =
        image::load_from_memory_with_format(raw_img, image::ImageFormat::Ico)
            .ok();
    if let Some(img) = img {
        let height = img.height();
        let width = img.width();
        let icon =
            iced::window::icon::from_rgba(img.into_bytes(), width, height).ok();
        settings.window.icon = icon;
    }
    debug!("Setup window");
    Helper::run(settings)
}

struct Helper {
    servers: Servers,
    current_view: View,
    login_state: LoginState,
    config: Config,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum View {
    Account {
        ident: AccountIdent,
        page: AccountPage,
    },
    Overview,
    Login,
    Settings,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum AccountPage {
    Scrapbook,
    Underworld,
}

fn get_server_code(server: &str) -> String {
    let server = server.trim_start_matches("https:");
    let server = server.trim_start_matches("http:");
    let server = server.replace('/', "");
    let mut parts = server.split('.');
    let a = parts.next();
    _ = parts.next();
    let b = parts.next();

    match (a, b) {
        (Some(a), Some(b)) => {
            format!("{a}.{b}")
        }
        _ => String::new(),
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct CharacterInfo {
    equipment: Vec<EquipmentIdent>,
    name: String,
    uid: u32,
    level: u16,
    #[serde(skip)]
    stats: Option<u32>,
    #[serde(skip)]
    fetch_date: Option<NaiveDate>,
}

impl PartialOrd for CharacterInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CharacterInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.level.cmp(&other.level) {
            core::cmp::Ordering::Equal => {}
            ord => return ord.reverse(),
        }
        match self.name.cmp(&other.name) {
            core::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        self.uid.cmp(&other.uid)
    }
}

impl Application for Helper {
    type Executor = executor::Default;

    type Message = Message;

    type Theme = Theme;

    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, iced::Command<Self::Message>) {
        let config = match Config::restore() {
            Ok(c) => c,
            Err(_) => {
                let def = Config::default();
                _ = def.write();
                def
            }
        };
        let helper = Helper {
            servers: Default::default(),
            login_state: LoginState {
                login_typ: if config.accounts.is_empty() {
                    LoginType::Regular
                } else {
                    LoginType::Saved
                },
                name: String::new(),
                password: String::new(),
                server: "f1.sfgame.net".to_string(),
                error: None,
                remember_me: true,
                active_sso: vec![],
                import_que: vec![],
                google_sso: Arc::new(Mutex::new(SSOStatus::Initializing)),
                steam_sso: Arc::new(Mutex::new(SSOStatus::Initializing)),
            },
            config,
            current_view: View::Login,
        };
        // TODO: Fetch update?
        (helper, Command::none())
    }

    fn theme(&self) -> Theme {
        self.config.theme.theme()
    }

    fn title(&self) -> String {
        format!("Scrapbook Helper v{}", env!("CARGO_PKG_VERSION"))
    }

    fn update(
        &mut self,
        message: Self::Message,
    ) -> iced::Command<Self::Message> {
        self.handle_msg(message)
    }

    fn view(
        &self,
    ) -> iced::Element<'_, Self::Message, Self::Theme, iced::Renderer> {
        self.view_current_page()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subs = vec![];

        for (server_id, server) in &self.servers.0 {
            for acc in server.accounts.values() {
                let subscription = subscription::unfold(
                    (acc.ident, 777),
                    AutoPoll {
                        player_status: acc.status.clone(),
                        ident: acc.ident,
                    },
                    move |a: AutoPoll| async move { (a.check().await, a) },
                );
                subs.push(subscription);

                if !acc.auto_battle {
                    continue;
                }
                let subscription = subscription::unfold(
                    (acc.ident, 69),
                    AutoAttackChecker {
                        player_status: acc.status.clone(),
                        ident: acc.ident,
                    },
                    move |a: AutoAttackChecker| async move { (a.check().await, a) },
                );
                subs.push(subscription);
            }

            if let CrawlingStatus::Crawling {
                crawling_session,
                threads,
                que,
                ..
            } = &server.crawling
            {
                let Some(session) = crawling_session else {
                    continue;
                };
                for thread in 0..*threads {
                    let subscription = subscription::unfold(
                        (thread, server.ident.id),
                        Crawler {
                            que: que.clone(),
                            state: session.clone(),
                            server_id: *server_id,
                        },
                        move |mut a: Crawler| async move { (a.crawl().await, a) },
                    );
                    subs.push(subscription);
                }
            }
        }

        for (arc, prov) in [
            (&self.login_state.steam_sso, SSOProvider::Steam),
            (&self.login_state.google_sso, SSOProvider::Google),
        ] {
            let arc = arc.clone();
            let subscription = subscription::unfold(
                prov,
                SSOValidator {
                    status: arc,
                    provider: prov,
                },
                move |a: SSOValidator| async move {
                    let msg = match a.check().await {
                        Ok(Some((chars, name))) => {
                            let chars = chars.into_iter().flatten().collect();
                            Message::SSOSuccess {
                                auth_name: name,
                                chars,
                                provider: prov,
                            }
                        }
                        Ok(None) => Message::SSORetry,
                        Err(e) => Message::SSOAuthError(e.to_string()),
                    };

                    (msg, a)
                },
            );
            subs.push(subscription);
        }

        Subscription::batch(subs)
    }
}

impl Helper {
    fn has_accounts(&self) -> bool {
        self.servers.0.iter().any(|a| !a.1.accounts.is_empty())
    }

    fn update_best(
        &mut self,
        ident: AccountIdent,
        keep_recent: bool,
    ) -> Command<Message> {
        trace!("Updating best for {ident:?} - keep recent: {keep_recent}");
        let Some(server) = self.servers.get_mut(&ident.server_id) else {
            return Command::none();
        };

        let CrawlingStatus::Crawling {
            que,
            threads,
            player_info,
            equipment,
            ..
        } = &mut server.crawling
        else {
            return Command::none();
        };

        let Some(account) = server.accounts.get_mut(&ident.account) else {
            return Command::none();
        };

        if keep_recent
            && account.last_updated + Duration::from_millis(500) >= Local::now()
        {
            return Command::none();
        }

        let per_player_counts = calc_per_player_count(
            player_info, equipment, &account.scrapbook.items, account,
        );
        let best_players = find_best(&per_player_counts, player_info, 20);

        account.best = best_players;
        account.last_updated = Local::now();

        let mut lock = que.lock().unwrap();
        for target in &account.best {
            if target.is_old()
                && !lock.todo_accounts.contains(&target.info.name)
                && !lock.invalid_accounts.contains(&target.info.name)
                && !lock.in_flight_accounts.contains(&target.info.name)
            {
                lock.todo_accounts.push(target.info.name.to_string())
            }
        }
        drop(lock);

        if *threads == 0 {
            return server.set_threads(1, &self.config.base_name);
        }
        Command::none()
    }
}

pub fn calc_per_player_count(
    player_info: &HashMap<
        u32,
        CharacterInfo,
        std::hash::BuildHasherDefault<nohash_hasher::NoHashHasher<u32>>,
    >,
    equipment: &HashMap<
        EquipmentIdent,
        HashSet<u32, ahash::RandomState>,
        ahash::RandomState,
    >,
    scrapbook: &HashSet<EquipmentIdent>,
    account: &AccountInfo,
) -> IntMap<u32, usize> {
    let mut per_player_counts = IntMap::default();
    per_player_counts.reserve(player_info.len());

    for (eq, players) in equipment.iter() {
        if scrapbook.contains(eq) || eq.model_id >= 100 {
            continue;
        }
        for player in players.iter() {
            *per_player_counts.entry(*player).or_insert(0) += 1;
        }
    }

    per_player_counts.retain(|a, _| {
        let Some(info) = player_info.get(a) else {
            return false;
        };
        if info.level > account.max_level {
            return false;
        }
        if let Some((_, lost)) = account.blacklist.get(&info.uid) {
            if *lost >= 5 {
                return false;
            }
        }
        true
    });
    per_player_counts
}

macro_rules! impl_unique_id {
    ($type:ty) => {
        impl $type {
            fn new() -> Self {
                static COUNTER: AtomicU64 = AtomicU64::new(0);
                Self(COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
            }
        }
    };
}

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
struct ServerID(u64);

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
pub struct QueID(u64);
impl_unique_id!(QueID);

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
struct AccountID(u64);
impl_unique_id!(AccountID);

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
pub struct AccountIdent {
    server_id: ServerID,
    account: AccountID,
}

impl ServerInfo {
    pub fn set_threads(
        &mut self,
        new_count: usize,
        base_name: &str,
    ) -> Command<Message> {
        let CrawlingStatus::Crawling {
            threads,
            crawling_session,
            ..
        } = &mut self.crawling
        else {
            return Command::none();
        };

        let not_logged_in = *threads == 0 && crawling_session.is_none();

        *threads = new_count;

        let base_name = base_name.to_string();
        let con = self.connection.clone();
        let id = self.ident.id;

        if not_logged_in {
            Command::perform(
                CrawlerState::try_login(base_name, con),
                move |res| match res {
                    Ok(state) => Message::CrawlerStartup {
                        server: id,
                        state: Arc::new(state),
                    },
                    Err(err) => Message::CrawlerDied {
                        server: id,
                        error: err.to_string(),
                    },
                },
            )
        } else {
            Command::none()
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct AttackTarget {
    missing: usize,
    info: CharacterInfo,
}
impl AttackTarget {
    fn is_old(&self) -> bool {
        self.info.fetch_date.unwrap_or_default() < Utc::now().date_naive()
    }
}

fn find_best(
    per_player_counts: &IntMap<u32, usize>,
    player_info: &IntMap<u32, CharacterInfo>,
    max_out: usize,
) -> Vec<AttackTarget> {
    // Prune the counts to make computation faster
    let mut max = 1;
    let mut counts = [(); 10].map(|_| vec![]);
    for (player, count) in per_player_counts.iter().map(|a| (*a.0, *a.1)) {
        if max_out == 1 && count < max || count == 0 {
            continue;
        }
        max = max.max(count);
        counts[(count - 1).clamp(0, 9)].push(player);
    }

    let mut best_players = Vec::new();
    for (count, players) in counts.iter().enumerate().rev() {
        best_players.extend(
            players.iter().flat_map(|a| player_info.get(a)).map(|a| {
                AttackTarget {
                    missing: count + 1,
                    info: a.to_owned(),
                }
            }),
        );
        if best_players.len() >= max_out {
            break;
        }
    }
    best_players.sort_by(|a, b| b.cmp(a));
    best_players.truncate(max_out);

    best_players
}

fn top_bar(
    center: Element<Message>,
    back: Option<Message>,
) -> Element<Message> {
    let back_button: Element<Message> = if let Some(back) = back {
        button("Back")
            .padding(4)
            .style(theme::Button::Destructive)
            .on_press(back)
            .into()
    } else {
        text("").into()
    };

    let back_button = container(back_button).width(Length::Fixed(100.0));

    let settings = container(
        button("Settings")
            .padding(4)
            .on_press(Message::ViewSettings),
    )
    .width(Length::Fixed(100.0))
    .align_x(iced::alignment::Horizontal::Right);

    row!(
        back_button,
        horizontal_space(),
        center,
        horizontal_space(),
        settings
    )
    .align_items(Alignment::Center)
    .padding(15)
    .into()
}

pub fn handle_new_char_info(
    char: CharacterInfo,
    equipment: &mut HashMap<
        EquipmentIdent,
        HashSet<u32, ahash::RandomState>,
        ahash::RandomState,
    >,
    player_info: &mut IntMap<u32, CharacterInfo>,
) {
    let player_entry = player_info.entry(char.uid);

    match player_entry {
        Entry::Occupied(mut old) => {
            // We have already seen this player. We have to remove the old info
            // and add the updated info
            let old_info = old.get();
            for eq in &old_info.equipment {
                if let Some(x) = equipment.get_mut(eq) {
                    x.remove(&old_info.uid);
                }
            }
            for eq in char.equipment.clone() {
                equipment
                    .entry(eq)
                    .and_modify(|a| {
                        a.insert(char.uid);
                    })
                    .or_insert_with(|| {
                        HashSet::from_iter([char.uid].into_iter())
                    });
            }
            old.insert(char);
        }
        Entry::Vacant(v) => {
            for eq in char.equipment.clone() {
                equipment
                    .entry(eq)
                    .and_modify(|a| {
                        a.insert(char.uid);
                    })
                    .or_insert_with(|| {
                        HashSet::from_iter([char.uid].into_iter())
                    });
            }
            v.insert(char);
        }
    }
}

fn get_log_config() -> log4rs::Config {
    let pattern = PatternEncoder::new(
        "{d(%Y-%m-%d %H:%M:%S)} | {({l}):5.5} | {M}:{L} | {m}{n}",
    );
    let stderr = ConsoleAppender::builder()
        .target(Target::Stderr)
        .encoder(Box::new(pattern.clone()))
        .build();

    let logfile = FileAppender::builder()
        .encoder(Box::new(pattern.clone()))
        .build("helper.log")
        .unwrap();

    let config = log4rs::Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .appender(Appender::builder().build("stderr", Box::new(stderr)))
        .logger(
            Logger::builder()
                .appender("logfile")
                .build("sf_scrapbook_helper", log::LevelFilter::Debug),
        )
        .logger(
            Logger::builder()
                .appender("logfile")
                .build("sf_api", log::LevelFilter::Warn),
        )
        .build(
            Root::builder()
                .appender("stderr")
                .build(log::LevelFilter::Error),
        )
        .unwrap();
    config
}
