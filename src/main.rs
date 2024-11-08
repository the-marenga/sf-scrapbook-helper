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
    collections::{hash_map::Entry, BTreeMap, HashMap, HashSet},
    sync::{atomic::AtomicU64, Arc, Mutex},
    time::Duration,
};

use chrono::{Local, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use config::{AccountConfig, Config};
use crawler::{CrawlAction, Crawler, CrawlerState, CrawlingOrder, WorkerQue};
use iced::{
    executor, subscription, theme,
    widget::{button, container, horizontal_space, row, text},
    Alignment, Application, Command, Element, Length, Settings, Subscription,
    Theme,
};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::{debug, info, trace};
use log4rs::{
    append::{
        console::{ConsoleAppender, Target},
        file::FileAppender,
    },
    config::{Appender, Logger, Root},
    encode::pattern::PatternEncoder,
};
use login::{LoginState, LoginType, PlayerAuth, SSOStatus, SSOValidator};
use nohash_hasher::{IntMap, IntSet};
use player::{
    AccountInfo, AccountStatus, AutoAttackChecker, AutoLureChecker, AutoPoll,
    ScrapbookInfo,
};
use serde::{Deserialize, Serialize};
use server::{CrawlingStatus, ServerIdent, ServerInfo, Servers};
use sf_api::{
    gamestate::{character::Class, unlockables::EquipmentIdent},
    session::ServerConnection,
    sso::{SSOProvider, ServerLookup},
};
use tokio::time::sleep;

use crate::{
    config::{AccountCreds, AvailableTheme},
    message::Message,
};
pub const PER_PAGE: usize = 51;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    pub sub: Option<CLICommand>,
}

#[derive(Debug, Subcommand, Clone)]
enum CLICommand {
    Crawl {
        /// The amount of servers that will be simultaniously crawled
        #[arg(short, long, default_value_t = 4, value_parser=concurrency_limits)]
        concurrency: usize,
        /// The amount of threads per server used to
        #[arg(short, long, default_value_t = 1, value_parser=concurrency_limits)]
        threads: usize,
        #[clap(flatten)]
        servers: ServerSelect,
    },
}
fn concurrency_limits(s: &str) -> Result<usize, String> {
    clap_num::number_range(s, 1, 50)
}

#[derive(Debug, clap::Args, Clone)]
#[group(required = true, multiple = false)]
pub struct ServerSelect {
    /// Fetches a list of all servers and crawls all of them. Supercedes urls
    #[arg(short, long)]
    all: bool,
    /// The list of all server urls to fetch
    #[arg(short, long, value_delimiter = ' ', num_args = 1..)]
    urls: Option<Vec<String>>,
}

impl Args {
    pub fn is_headless(&self) -> bool {
        self.sub.is_some()
    }
}

fn main() -> iced::Result {
    let args = Args::parse();

    let is_headless = args.is_headless();
    let config = get_log_config(is_headless);
    log4rs::init_config(config).unwrap();
    info!("Starting up");

    let mut settings = Settings::with_flags(args);
    settings.window.min_size = Some(iced::Size {
        width: 700.0,
        height: 700.0,
    });
    settings.default_text_size = 13.0f32.into();
    settings.window.visible = !is_headless;

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
    Helper::run(settings)
}

struct Helper {
    servers: Servers,
    current_view: View,
    login_state: LoginState,
    config: Config,
    should_update: bool,
    class_images: ClassImages,
    cli_crawling: Option<CLICrawling>,
}

struct CLICrawling {
    todo_servers: Vec<String>,
    mbp: MultiProgress,
    threads: usize,
    active: usize,
}

struct ClassImages {
    assassin: iced::widget::image::Handle,
    bard: iced::widget::image::Handle,
    berserk: iced::widget::image::Handle,
    battle_mage: iced::widget::image::Handle,
    demon_hunter: iced::widget::image::Handle,
    druid: iced::widget::image::Handle,
    necromancer: iced::widget::image::Handle,
    scout: iced::widget::image::Handle,
    warrior: iced::widget::image::Handle,
    mage: iced::widget::image::Handle,
}

macro_rules! load_class_image {
    ($path:expr) => {{
        let raw_img = include_bytes!($path);
        let image = image::load_from_memory_with_format(
            raw_img,
            image::ImageFormat::WebP,
        )
        .unwrap();
        iced::widget::image::Handle::from_pixels(
            image.width(),
            image.height(),
            image.into_bytes(),
        )
    }};
}

impl ClassImages {
    pub fn new() -> ClassImages {
        ClassImages {
            assassin: load_class_image!("../assets/classes/assassin.webp"),
            bard: load_class_image!("../assets/classes/bard.webp"),
            berserk: load_class_image!("../assets/classes/berserk.webp"),
            demon_hunter: load_class_image!(
                "../assets/classes/demon_hunter.webp"
            ),
            druid: load_class_image!("../assets/classes/druid.webp"),
            necromancer: load_class_image!(
                "../assets/classes/necromancer.webp"
            ),
            scout: load_class_image!("../assets/classes/scout.webp"),
            warrior: load_class_image!("../assets/classes/warrior.webp"),
            mage: load_class_image!("../assets/classes/mage.webp"),
            battle_mage: load_class_image!(
                "../assets/classes/battle_mage.webp"
            ),
        }
    }

    pub fn get_handle(&self, class: Class) -> iced::widget::image::Handle {
        match class {
            Class::Warrior => self.warrior.clone(),
            Class::Mage => self.mage.clone(),
            Class::Scout => self.scout.clone(),
            Class::Assassin => self.assassin.clone(),
            Class::BattleMage => self.battle_mage.clone(),
            Class::Berserker => self.berserk.clone(),
            Class::DemonHunter => self.demon_hunter.clone(),
            Class::Druid => self.druid.clone(),
            Class::Bard => self.bard.clone(),
            Class::Necromancer => self.necromancer.clone(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum View {
    Account {
        ident: AccountIdent,
        page: AccountPage,
    },
    Overview {
        selected: HashSet<AccountIdent>,
        action: Option<ActionSelection>,
    },
    Login,
    Settings,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ActionSelection {
    Multi,
    Character(AccountIdent),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum AccountPage {
    Scrapbook,
    Underworld,
    Options,
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
    #[serde(skip)]
    class: Option<Class>,
}

impl CharacterInfo {
    pub fn is_old(&self) -> bool {
        self.fetch_date.unwrap_or_default() < Utc::now().date_naive()
    }
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

    type Flags = Args;

    fn new(flags: Args) -> (Self, iced::Command<Self::Message>) {
        let config = Config::restore().unwrap_or_default();
        let mut helper = Helper {
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
            current_view: View::Login,
            should_update: false,
            class_images: ClassImages::new(),
            config,
            cli_crawling: None,
        };

        let fetch_update =
            Command::perform(async { check_update().await }, |res| {
                Message::UpdateResult(res.unwrap_or_default())
            });
        let mut commands = vec![fetch_update];

        if let Some(CLICommand::Crawl {
            concurrency,
            threads,
            servers,
        }) = flags.sub
        {
            let mut info = CLICrawling {
                todo_servers: Vec::new(),
                mbp: MultiProgress::new(),
                active: concurrency,
                threads,
            };

            if let Some(servers) = servers.urls {
                info.todo_servers = servers;

                for _ in 0..concurrency {
                    commands.push(Command::perform(async {}, move |_| {
                        Message::NextCLICrawling
                    }))
                }
            } else if servers.all {
                let c = Command::perform(
                    async {
                        ServerLookup::fetch().await.ok().map(|a| {
                            a.all()
                                .into_iter()
                                .map(|a| a.to_string())
                                .filter(|a| a != "https://speed.sfgame.net/")
                                .collect()
                        })
                    },
                    move |servers| Message::CrawlAllRes {
                        servers,
                        concurrency,
                    },
                );
                commands.push(c);
            }
            helper.cli_crawling = Some(info);
        }
        commands.push(
            iced::font::load(iced_aw::BOOTSTRAP_FONT_BYTES)
                .map(Message::FontLoaded),
        );

        let mut loading = 0;

        for acc in &helper.config.accounts {
            match acc {
                AccountConfig::Regular { config, .. } => {
                    if config.login {
                        let acc = acc.clone();
                        loading += 1;
                        commands.push(Command::perform(
                            async move {
                                sleep(Duration::from_millis(
                                    (loading - 1) * 200,
                                ))
                                .await
                            },
                            move |_| Message::Login {
                                account: acc,
                                auto_login: true,
                            },
                        ));
                    }
                }
                AccountConfig::SF { characters, .. } => {
                    if characters.iter().any(|a| a.config.login) {
                        loading += 1;
                        let acc = acc.clone();
                        commands.push(Command::perform(
                            async move {
                                sleep(Duration::from_millis(
                                    (loading - 1) * 200,
                                ))
                                .await
                            },
                            move |_| Message::Login {
                                account: acc,
                                auto_login: true,
                            },
                        ));
                    }
                }
            }
        }

        if loading > 0 {
            helper.current_view = View::Overview {
                selected: Default::default(),
                action: Default::default(),
            };
        }

        (helper, Command::batch(commands))
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
        // let start = std::time::Instant::now();
        // let msg = format!("{message:?}");
        let res = self.handle_msg(message);
        _ = &res;
        // let time = start.elapsed();
        // if true{
        //     println!(
        //         "{} took: {time:?}",
        //         msg.split('{').next().unwrap_or(&msg).trim(),
        //     );
        // }
        res
    }

    fn view(
        &self,
    ) -> iced::Element<'_, Self::Message, Self::Theme, iced::Renderer> {
        self.view_current_page()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        // Disambiguates running subscriptions
        #[derive(Debug, Hash, PartialEq, Eq)]
        enum SubIdent {
            RefreshUI,
            AutoPoll(AccountIdent),
            AutoBattle(AccountIdent),
            AutoLure(AccountIdent),
            SSOCheck(SSOProvider),
            Crawling(usize, ServerID),
        }

        let mut subs = vec![];
        let subscription = subscription::unfold(
            SubIdent::RefreshUI,
            (),
            move |a: ()| async move {
                sleep(Duration::from_millis(200)).await;
                (Message::UIActive, a)
            },
        );
        subs.push(subscription);

        for (server_id, server) in &self.servers.0 {
            for acc in server.accounts.values() {
                if self.config.auto_poll {
                    let subscription = subscription::unfold(
                        SubIdent::AutoPoll(acc.ident),
                        AutoPoll {
                            player_status: acc.status.clone(),
                            ident: acc.ident,
                        },
                        move |a: AutoPoll| async move { (a.check().await, a) },
                    );
                    subs.push(subscription);
                }

                if let Some(si) = &acc.scrapbook_info {
                    if si.auto_battle {
                        let subscription = subscription::unfold(
                            SubIdent::AutoBattle(acc.ident),
                            AutoAttackChecker {
                                player_status: acc.status.clone(),
                                ident: acc.ident,
                            },
                            move |a: AutoAttackChecker| async move {
                                (a.check().await, a)
                            },
                        );
                        subs.push(subscription);
                    }
                };

                if let Some(ui) = &acc.underworld_info {
                    if ui.auto_lure {
                        let subscription = subscription::unfold(
                            SubIdent::AutoLure(acc.ident),
                            AutoLureChecker {
                                player_status: acc.status.clone(),
                                ident: acc.ident,
                            },
                            move |a: AutoLureChecker| async move {
                                (a.check().await, a)
                            },
                        );
                        subs.push(subscription);
                    }
                }
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
                        SubIdent::Crawling(thread, server.ident.id),
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
                SubIdent::SSOCheck(prov),
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
                        Err(e) => Message::SSOAuthError {
                            _error: e.to_string(),
                        },
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
    fn force_init_crawling(
        &mut self,
        url: &str,
        threads: usize,
        pb: ProgressBar,
    ) -> Option<Command<Message>> {
        let ident = ServerIdent::new(url);
        let connection = ServerConnection::new(url)?;
        pb.enable_steady_tick(Duration::from_millis(30));
        pb.set_prefix(ident.ident.to_string());
        set_full_bar(&pb, "Crawling", 0);
        let server = self.servers.get_or_insert_default(
            ident,
            connection,
            Some(pb.clone()),
        );

        let que_id = QueID::new();

        let que = WorkerQue {
            que_id,
            todo_pages: Default::default(),
            todo_accounts: Default::default(),
            invalid_pages: Default::default(),
            invalid_accounts: Default::default(),
            in_flight_pages: Default::default(),
            in_flight_accounts: Default::default(),
            order: Default::default(),
            lvl_skipped_accounts: Default::default(),
            min_level: Default::default(),
            max_level: 9999,
            self_init: true,
        };

        server.crawling = CrawlingStatus::Crawling {
            que_id,
            threads: 0,
            que: Arc::new(Mutex::new(que)),
            player_info: Default::default(),
            equipment: Default::default(),
            naked: Default::default(),
            last_update: Local::now(),
            crawling_session: None,
            recent_failures: Default::default(),
        };
        Some(server.set_threads(threads, &self.config.base_name))
    }

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
            naked,
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

        let mut has_old = false;

        let mut lock = que.lock().unwrap();
        let invalid =
            lock.invalid_accounts.iter().map(|a| a.as_str()).collect();

        let result_limit = 50;

        if let Some(si) = &mut account.scrapbook_info {
            let per_player_counts = calc_per_player_count(
                player_info, equipment, &si.scrapbook.items, si,
                self.config.blacklist_threshold,
            );
            let mut best_players = find_best(
                &per_player_counts, player_info, result_limit, &invalid,
            );

            best_players.sort_by(|a, b| {
                b.missing
                    .cmp(&a.missing)
                    .then(a.info.stats.cmp(&b.info.stats))
                    .then(a.info.level.cmp(&b.info.level))
            });

            si.best = best_players;

            for target in &si.best {
                if target.is_old()
                    && !lock.todo_accounts.contains(&target.info.name)
                    && !lock.invalid_accounts.contains(&target.info.name)
                    && !lock.in_flight_accounts.contains(&target.info.name)
                {
                    has_old = true;
                    lock.todo_accounts.push(target.info.name.to_string())
                }
            }
        };

        if let Some(ui) = &mut account.underworld_info {
            ui.best.clear();
            'a: for (_, players) in naked.range(..=ui.max_level).rev() {
                for player in players.iter() {
                    if ui.best.len() >= result_limit {
                        break 'a;
                    }
                    let Some(info) = player_info.get(player) else {
                        continue;
                    };
                    if info.is_old()
                        && !lock.todo_accounts.contains(&info.name)
                        && !lock.invalid_accounts.contains(&info.name)
                        && !lock.in_flight_accounts.contains(&info.name)
                    {
                        has_old = true;
                        lock.todo_accounts.push(info.name.to_string())
                    }
                    ui.best.push(info.to_owned());
                }
            }
        }
        drop(lock);

        account.last_updated = Local::now();

        if (has_old || player_info.is_empty()) && *threads == 0 {
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
    si: &ScrapbookInfo,
    blacklist_th: usize,
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

        if info.level > si.max_level {
            return false;
        }

        if info.stats.unwrap_or_default() > si.max_attributes {
            return false;
        }

        if let Some((_, lost)) = si.blacklist.get(&info.uid) {
            if *lost >= blacklist_th.max(1) {
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
pub struct ServerID(u64);

impl std::fmt::Display for ServerID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("server-{}", self.0))
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
pub struct QueID(u64);
impl_unique_id!(QueID);

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
pub struct AccountID(u64);
impl_unique_id!(AccountID);

impl std::fmt::Display for AccountID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("character-{}", self.0))
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
pub struct AccountIdent {
    server_id: ServerID,
    account: AccountID,
}

impl std::fmt::Display for AccountIdent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "character-{}@{}",
            self.account.0, self.server_id.0
        ))
    }
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
        self.info.is_old()
    }
}

fn find_best(
    per_player_counts: &IntMap<u32, usize>,
    player_info: &IntMap<u32, CharacterInfo>,
    max_out: usize,
    invalid: &HashSet<&str>,
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
            players
                .iter()
                .flat_map(|a| player_info.get(a))
                .filter(|a| !invalid.contains(&a.name.as_str()))
                .map(|a| AttackTarget {
                    missing: count + 1,
                    info: a.to_owned(),
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
    naked: &mut BTreeMap<u16, IntSet<u32>>,
) {
    let player_entry = player_info.entry(char.uid);

    const EQ_CUTOFF: usize = 4;

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
            if old_info.equipment.len() < EQ_CUTOFF {
                naked.entry(old_info.level).and_modify(|a| {
                    a.remove(&old_info.uid);
                });
            }

            if char.equipment.len() < EQ_CUTOFF {
                naked.entry(char.level).or_default().insert(char.uid);
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
            if char.equipment.len() < EQ_CUTOFF && char.level >= 100 {
                naked.entry(char.level).or_default().insert(char.uid);
            }
            v.insert(char);
        }
    }
}

fn get_log_config(is_headless: bool) -> log4rs::Config {
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

    let mut logger = log4rs::Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)));
    let mut root = Root::builder();

    if !is_headless {
        logger = logger
            .appender(Appender::builder().build("stderr", Box::new(stderr)));
        root = root.appender("stderr");
    }

    logger
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
        .build(root.build(log::LevelFilter::Error))
        .unwrap()
}

async fn check_update() -> Result<bool, Box<dyn std::error::Error>> {
    sleep(Duration::from_millis(fastrand::u64(500..=5000))).await;
    let client = reqwest::ClientBuilder::new()
        .user_agent("sf-scrapbook-helper")
        .build()?;
    let url =
        "https://api.github.com/repos/the-marenga/sf-scrapbook-helper/tags";
    let resp = client.get(url).send().await?;

    let text = resp.text().await?;

    #[derive(Debug, Deserialize)]
    struct GitTag {
        name: String,
    }

    let tags: Vec<GitTag> = serde_json::from_str(&text)?;

    let mut should_update = false;
    if let Some(newest) = tags.first() {
        let git_version =
            semver::Version::parse(newest.name.trim_start_matches('v'))?;
        let own_version = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
        should_update = own_version < git_version;
    }
    Ok(should_update)
}

pub fn set_full_bar(bar: &ProgressBar, title: &str, length: usize) {
    let style = ProgressStyle::default_spinner()
        .template(
            "{spinner} {prefix:17.red} - {msg:25.blue} {wide_bar:.green} \
             [{elapsed_precise}/{duration_precise}] [{pos:6}/{len:6}]",
        )
        .unwrap_or_else(|_| ProgressStyle::default_spinner());

    bar.set_style(style);
    bar.reset_elapsed();
    bar.set_message(title.to_string());
    bar.set_length(length as u64);
    bar.set_position(0);
}
