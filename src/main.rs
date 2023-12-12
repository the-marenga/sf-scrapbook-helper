#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, Sender, TryRecvError},
        Arc, Mutex,
    },
    time::Duration,
};

use chrono::Local;
use eframe::{
    egui::{self, CentralPanel, Context, Layout, SidePanel},
    epaint::ahash::{HashMap, HashSet},
};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use sf_api::{
    command::Command,
    error::SFError,
    gamestate::{
        character::{Class, Gender, Race},
        unlockables::{EquipmentIdent, ScrapBook},
        GameState,
    },
    session::{CharacterSession, Response, ServerConnection},
    sso::SFAccount,
};
use tokio::{runtime::Runtime, sync::mpsc::UnboundedSender, task::JoinHandle};

fn main() -> Result<(), eframe::Error> {
    let rt = Runtime::new().expect("Unable to create Runtime");
    let _enter = rt.enter();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default(),
        ..Default::default()
    };
    eframe::run_native(
        "Scrapbook Helper v0.1",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_pixels_per_point(1.4);
            Box::new(Stage::start_page(None))
        }),
    )
}

enum Stage {
    Login {
        name: String,
        password: String,
        server: String,
        sso_name: String,
        sso_password: String,
        error: Option<String>,
    },
    LoggingIn(
        Arc<Mutex<Option<(Result<Response, SFError>, CharacterSession)>>>,
        JoinHandle<()>,
        ServerConnection,
    ),
    Overview {
        gs: Arc<Mutex<GameState>>,
        observ_sender: Sender<ObserverCommand>,
        observ_receiver: Receiver<ObserverInfo>,
        active: usize,
        last_response: ObserverInfo,
        max_level: u16,

        player_sender: Sender<PlayerCommand>,
        player_receiver: Receiver<PlayerInfo>,

        last_player_response: Option<PlayerInfo>,
        auto_battle: bool,
        server_url: String,
    },
    SSOLoggingIn(
        Arc<Mutex<Option<Result<Vec<CharacterSession>, SFError>>>>,
        JoinHandle<()>,
    ),
    SSODecide(Vec<CharacterSession>),
}

impl Stage {
    pub fn start_page(error: Option<String>) -> Stage {
        Stage::Login {
            name: "".to_owned(),
            password: "".to_owned(),
            server: "s1.sfgame.de".to_owned(),
            sso_name: "".to_string(),
            sso_password: "".to_string(),
            error,
        }
    }
}

enum PlayerInfo {
    Victory { name: String, uid: u32 },
    Lost { name: String },
}

enum PlayerCommand {
    Attack { name: String, uid: u32, mush: bool },
}

pub struct ObserverInfo {
    best_players: Vec<(usize, CharacterInfo)>,
}

static CONTEXT: OnceCell<Context> = OnceCell::new();

impl eframe::App for Stage {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        CONTEXT.get_or_init(|| ctx.to_owned());
        let mut new_stage = None;
        egui::CentralPanel::default().show(ctx, |ui| match self {
            Stage::Login {
                name,
                password,
                server,
                sso_name,
                sso_password,
                error,
            } => {
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    ui.heading("Regular Login");
                    ui.add_space(12.0);

                    ui.horizontal(|ui| {
                        let name_label = ui.label("Username: ");
                        ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::singleline(name),
                        )
                        .labelled_by(name_label.id);
                    });
                    ui.horizontal(|ui| {
                        let password_label = ui.label("Password: ");
                        ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::singleline(password).password(true),
                        )
                        .labelled_by(password_label.id);
                    });
                    ui.horizontal(|ui| {
                        let server_label = ui.label("Server: ");
                        ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::singleline(server),
                        )
                        .labelled_by(server_label.id);
                    });
                    ui.add_space(12.0);

                    if ui.button("Login").clicked() {
                        let Some(sc) = ServerConnection::new(server) else {
                            *error = Some("Invalid Server URL".to_string());
                            return;
                        };

                        let session = sf_api::session::CharacterSession::new(
                            name,
                            password,
                            sc.clone(),
                        );

                        let arc = Arc::new(Mutex::new(None));
                        let arc2 = arc.clone();

                        let handle = tokio::spawn(async move {
                            let mut session = session;
                            let res = session.login().await;
                            *arc2.lock().unwrap() = Some((res, session));
                            let c = CONTEXT.get().unwrap();
                            c.request_repaint();
                        });
                        new_stage = Some(Stage::LoggingIn(arc, handle, sc));
                    }
                    ui.add_space(12.0);
                    ui.heading("SSO Login");
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        let name_label = ui.label("Username: ");
                        ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::singleline(sso_name),
                        )
                        .labelled_by(name_label.id);
                    });
                    ui.horizontal(|ui| {
                        let password_label = ui.label("Password: ");
                        ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::singleline(sso_password)
                                .password(true),
                        )
                        .labelled_by(password_label.id);
                    });

                    if ui.button("SSO Login").clicked() {
                        let arc = Arc::new(Mutex::new(None));
                        let output = arc.clone();

                        let username = sso_name.clone();
                        let password = sso_password.clone();

                        let handle = tokio::spawn(async move {
                            let account = match SFAccount::login(
                                username, password,
                            )
                            .await
                            {
                                Ok(account) => account,
                                Err(err) => {
                                    *output.lock().unwrap() = Some(Err(err));
                                    return;
                                }
                            };

                            match account.characters().await {
                                Ok(character) => {
                                    let vec = character
                                        .into_iter()
                                        .flatten()
                                        .collect::<Vec<_>>();
                                    *output.lock().unwrap() = Some(Ok(vec));
                                }
                                Err(err) => {
                                    *output.lock().unwrap() = Some(Err(err));
                                }
                            };
                            let c = CONTEXT.get().unwrap();
                            c.request_repaint();
                        });

                        new_stage = Some(Stage::SSOLoggingIn(arc, handle));
                    }
                    ui.add_space(12.0);

                    if let Some(error) = &error {
                        ui.label(error);
                    }
                });
            }
            Stage::SSODecide(character) => {
                ui.vertical_centered(|ui| {
                    for session in character {
                        if ui
                            .button(format!(
                                "{} - {}",
                                session.username(),
                                session.server_url().as_str()
                            ))
                            .clicked()
                        {
                            let mut session = session.clone();
                            let connection = ServerConnection::new(
                                session.server_url().as_str(),
                            )
                            .unwrap();

                            let arc = Arc::new(Mutex::new(None));
                            let arc2 = arc.clone();

                            let handle = tokio::spawn(async move {
                                let res = session.login().await;
                                *arc2.lock().unwrap() = Some((res, session));
                                let c = CONTEXT.get().unwrap();
                                c.request_repaint();
                            });

                            new_stage =
                                Some(Stage::LoggingIn(arc, handle, connection));
                        }
                    }
                });
            }
            Stage::SSOLoggingIn(arc, handle) => {
                let res = match arc.try_lock() {
                    Ok(mut r) => r.take(),
                    _ => {
                        ui.label("Logging in. Please wait...".to_string());
                        return;
                    }
                };
                match res {
                    Some(Err(error)) => {
                        handle.abort();
                        *self = Stage::start_page(Some(format!(
                            "Could not login: {error}"
                        )));
                    }
                    Some(Ok(character)) => {
                        handle.abort();
                        *self = Stage::SSODecide(character);
                    }
                    None => {
                        ui.with_layout(
                            Layout::centered_and_justified(
                                egui::Direction::TopDown,
                            ),
                            |ui| {
                                ui.label(
                                    "Logging in. Please wait...".to_string(),
                                );
                            },
                        );
                    }
                }
            }
            Stage::LoggingIn(response, handle, sc) => {
                let res = match response.try_lock() {
                    Ok(mut r) => r.take(),
                    _ => {
                        ui.label("Logging in. Please wait...".to_string());
                        return;
                    }
                };
                match res {
                    Some((Err(error), _)) => {
                        handle.abort();
                        *self = Stage::start_page(Some(format!(
                            "Could not login: {error}"
                        )));
                    }
                    Some((Ok(resp), session)) => {
                        handle.abort();

                        let gs = GameState::new(resp).unwrap();

                        let (cmd_sender, cmd_recv) = std::sync::mpsc::channel();
                        let (info_sender, info_recv) =
                            std::sync::mpsc::channel();

                        let initial_count = 1;

                        let Some(sb) = gs.unlocks.scrapbok.clone() else {
                            *self = Stage::start_page(Some(
                                "Player does not have a scrapbook".to_string(),
                            ));
                            return;
                        };

                        let mut hasher = twox_hash::XxHash64::with_seed(0);
                        gs.character.name.as_str().hash(&mut hasher);
                        session.server_url().as_str().hash(&mut hasher);
                        let player_hash = hasher.finish();

                        let mut hasher = twox_hash::XxHash64::with_seed(0);
                        session.server_url().as_str().hash(&mut hasher);
                        let server_hash = hasher.finish();

                        let server_url =
                            session.server_url().as_str().to_string();

                        tokio::spawn(observer(
                            info_sender,
                            cmd_recv,
                            sb,
                            sc.clone(),
                            gs.other_players.total_player as usize,
                            initial_count,
                            gs.character.level,
                            player_hash,
                            server_hash,
                        ));

                        let max_level = gs.character.level;
                        let (player_sender, player_recv) =
                            std::sync::mpsc::channel();
                        let (pi_sender, pi_recv) = std::sync::mpsc::channel();
                        let gs = Arc::new(Mutex::new(gs));

                        tokio::spawn(handle_player(
                            pi_sender,
                            player_recv,
                            session,
                            gs.clone(),
                        ));

                        *self = Stage::Overview {
                            max_level,
                            gs,
                            observ_sender: cmd_sender,
                            observ_receiver: info_recv,
                            active: initial_count,
                            last_response: ObserverInfo {
                                best_players: Vec::new(),
                            },
                            player_sender,
                            player_receiver: pi_recv,
                            last_player_response: None,
                            auto_battle: false,
                            server_url,
                        };
                    }
                    None => {
                        ui.with_layout(
                            Layout::centered_and_justified(
                                egui::Direction::TopDown,
                            ),
                            |ui| {
                                ui.label(
                                    "Logging in. Please wait...".to_string(),
                                );
                            },
                        );
                    }
                }
            }
            Stage::Overview {
                gs,
                observ_sender: sender,
                observ_receiver: receiver,
                active,
                last_response,
                max_level,
                player_sender,
                player_receiver,
                last_player_response,
                auto_battle,
                server_url,
            } => {
                if let Ok(resp) = receiver.try_recv() {
                    *last_response = resp
                }

                if let Ok(resp) = player_receiver.try_recv() {
                    match &resp {
                        PlayerInfo::Victory { uid, .. } => {
                            sender
                                .send(ObserverCommand::UpdateFight(*uid))
                                .unwrap();
                        }
                        PlayerInfo::Lost { .. } => {}
                    }
                    *last_player_response = Some(resp)
                }

                let Ok(mut gs) = gs.lock() else {
                    std::process::exit(1);
                };
                SidePanel::left("left").show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading(&gs.character.name.clone());

                        ui.label(format!(
                            "Mushrooms: {}",
                            gs.character.mushrooms
                        ));
                        ui.label(format!("Rank: {}", gs.character.rank));
                        ui.label(format!(
                            "Items found: {}",
                            gs.unlocks.scrapbook_count.unwrap_or_default()
                        ));

                        ui.label(format!(
                            "Pages fetched: {}",
                            PAGE_POS.fetch_add(0, Ordering::SeqCst)
                        ));

                        ui.label(format!(
                            "Fetched {}/{} players",
                            FETCHED_PLAYERS.fetch_add(0, Ordering::SeqCst),
                            TOTAL_PLAYERS.fetch_add(0, Ordering::SeqCst)
                        ));

                        ui.add_space(20.0);

                        egui::Grid::new("hof_grid").show(ui, |ui| {
                            ui.label("Crawl threads");
                            ui.add(
                                egui::DragValue::new(active)
                                    .clamp_range(1..=10),
                            );
                            if ui.button("Set").clicked() {
                                sender
                                    .send(ObserverCommand::SetAccounts(*active))
                                    .unwrap();
                            }
                            ui.end_row();
                            ui.label("Max target level");
                            ui.add(
                                egui::DragValue::new(max_level)
                                    .clamp_range(1..=800),
                            );
                            if ui.button("Set").clicked() {
                                sender
                                    .send(ObserverCommand::SetMaxLevel(
                                        *max_level,
                                    ))
                                    .unwrap();
                            }
                            ui.end_row();
                        });

                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            if ui.button("Pause Crawling").clicked() {
                                sender.send(ObserverCommand::Pause).unwrap()
                            }
                            if ui.button("Start Crawling").clicked() {
                                sender.send(ObserverCommand::Start).unwrap()
                            }
                        });

                        ui.add_space(20.0);

                        let mut free_fight_possible = false;
                        ui.label(match gs.arena.next_free_fight {
                            Some(t) if t > Local::now() => format!(
                                "Next free fight: {:?} sec",
                                (t - Local::now()).num_seconds()
                            ),
                            _ => {
                                free_fight_possible = true;
                                "Free fight possible".to_string()
                            }
                        });

                        ui.checkbox(auto_battle, "Auto Battle");

                        if let Some(last) = last_player_response {
                            ui.label(match last {
                                PlayerInfo::Victory { name, .. } => {
                                    format!("Won the last fight against {name}")
                                }
                                PlayerInfo::Lost { name } => {
                                    format!(
                                        "Lost the last fight against {name}"
                                    )
                                }
                            });
                        }
                        if *auto_battle && free_fight_possible {
                            if let Some((_, info)) =
                                last_response.best_players.first()
                            {
                                player_sender
                                    .send(PlayerCommand::Attack {
                                        name: info.name.clone(),
                                        uid: info.uid,
                                        mush: false,
                                    })
                                    .unwrap();
                            }
                            gs.arena.next_free_fight = Some(
                                Local::now() + Duration::from_secs(60 * 10),
                            );
                        }

                        ui.add_space(20.0);

                        if ui.button("Backup HoF").clicked() {
                            sender
                                .send(ObserverCommand::Export(
                                    server_url
                                        .trim_start_matches("https")
                                        .to_string(),
                                ))
                                .unwrap();
                        }

                        if ui.button("Restore HoF").clicked() {
                            sender
                                .send(ObserverCommand::Restore(
                                    server_url
                                        .trim_start_matches("https")
                                        .to_string(),
                                ))
                                .unwrap();
                        }

                        if ui.button("Export Player").clicked() {
                            _ = std::fs::write(
                                format!("{}.player", &gs.character.name),
                                serde_json::to_string_pretty(&gs.clone())
                                    .unwrap(),
                            );
                        }
                    });
                });
                CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("Possible Targets");
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            egui::Grid::new("hof_grid").show(ui, |ui| {
                                ui.label("Missing");
                                ui.label("Name");
                                ui.label("Level");
                                ui.label("Fight");
                                ui.end_row();
                                for (count, info) in &last_response.best_players
                                {
                                    ui.label(count.to_string());
                                    ui.label(&info.name);
                                    ui.label(info.level.to_string());
                                    if ui.button("Fight").clicked() {
                                        player_sender
                                            .send(PlayerCommand::Attack {
                                                name: info.name.clone(),
                                                uid: info.uid,
                                                mush: true,
                                            })
                                            .unwrap();
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    });
                });
            }
        });

        if let Some(stage) = new_stage {
            *self = stage;
        }
    }
}

enum ObserverCommand {
    SetAccounts(usize),
    SetMaxLevel(u16),
    UpdateFight(u32),
    Start,
    Pause,
    Export(String),
    Restore(String),
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
struct CharacterInfo {
    equipment: Vec<EquipmentIdent>,
    name: String,
    uid: u32,
    level: u16,
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

async fn observer(
    output: Sender<ObserverInfo>,
    receiver: Receiver<ObserverCommand>,
    mut target: ScrapBook,
    server: ServerConnection,
    total_players: usize,
    initial_count: usize,
    mut max_level: u16,
    player_hash: u64,
    server_hash: u64,
) {
    let mut player_info: HashMap<u32, CharacterInfo> = Default::default();
    let mut equipment: HashMap<EquipmentIdent, HashSet<u32>> =
        Default::default();
    let mut acccounts: Vec<(JoinHandle<()>, UnboundedSender<CrawlerCommand>)> =
        Vec::new();
    let initial_started = false;

    TOTAL_PLAYERS.fetch_add(total_players, Ordering::SeqCst);

    let total_pages = total_players / 30;
    let mut rng = fastrand::Rng::with_seed(player_hash);

    let (character_sender, mut character_receiver) =
        tokio::sync::mpsc::unbounded_channel();

    // We use the same bot accounts for the same user
    let mut base_name = rng.alphabetic().to_ascii_uppercase().to_string();
    for _ in 0..6 {
        let c = if rng.bool() {
            rng.alphabetic()
        } else {
            rng.digit(10)
        };
        base_name.push(c)
    }

    let mut rng = fastrand::Rng::with_seed(server_hash);
    let mut pages: Vec<usize> = (0..=total_pages).collect();
    rng.shuffle(&mut pages);

    for _ in 0..initial_count {
        let (sender, recv) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(crawl(
            recv,
            character_sender.clone(),
            initial_started,
            pages.clone(),
            server.clone(),
            format!("{base_name}{}", acccounts.len() + 7),
        ));
        acccounts.push((handle, sender));
    }

    let mut last_pc = 0;
    let mut last_ec = 0;

    loop {
        match receiver.try_recv() {
            Ok(data) => match data {
                ObserverCommand::UpdateFight(found) => {
                    target.items.extend(
                        player_info.get(&found).unwrap().equipment.iter(),
                    );
                }
                ObserverCommand::SetMaxLevel(max) => {
                    max_level = max;
                }
                ObserverCommand::SetAccounts(count) => {
                    while count > acccounts.len() {
                        let (sender, recv) =
                            tokio::sync::mpsc::unbounded_channel();
                        let cs = character_sender.clone();
                        let pages = pages.clone();
                        let server = server.clone();
                        let handle = tokio::spawn(crawl(
                            recv,
                            cs,
                            initial_started,
                            pages,
                            server,
                            format!("{base_name}{}", acccounts.len() + 7),
                        ));
                        acccounts.push((handle, sender));
                    }

                    for (_, sender) in &acccounts[0..(count - 1)] {
                        _ = sender.send(CrawlerCommand::Start);
                    }
                    for (_, sender) in &acccounts[count.saturating_sub(1)..] {
                        _ = sender.send(CrawlerCommand::Pause);
                    }
                }
                ObserverCommand::Start => {
                    for (_, sender) in &acccounts {
                        _ = sender.send(CrawlerCommand::Start);
                    }
                }
                ObserverCommand::Pause => {
                    for (_, sender) in &acccounts {
                        _ = sender.send(CrawlerCommand::Pause);
                    }
                }
                ObserverCommand::Export(name) => {
                    let normal = name
                        .chars()
                        .filter(|a| a.is_ascii_alphanumeric())
                        .collect::<String>();

                    let str = (
                        PAGE_POS.load(Ordering::SeqCst),
                        player_info
                            .iter()
                            .map(|a| a.1.clone())
                            .collect::<Vec<_>>(),
                    );
                    let str = serde_json::to_string_pretty(&str).unwrap();
                    _ = std::fs::write(&format!("{normal}.hof"), &str);
                }
                ObserverCommand::Restore(name) => {
                    let normal = name
                        .chars()
                        .filter(|a| a.is_ascii_alphanumeric())
                        .collect::<String>();

                    match std::fs::read_to_string(&format!("{normal}.hof")) {
                        Ok(text) => {
                            match serde_json::from_str::<(
                                usize,
                                Vec<CharacterInfo>,
                            )>(&text)
                            {
                                Ok((pos, chars)) => {
                                    for char in chars {
                                        handle_new_char_info(
                                            char, &mut equipment,
                                            &mut player_info,
                                        );
                                    }
                                    PAGE_POS.store(pos, Ordering::SeqCst);
                                    FETCHED_PLAYERS.store(
                                        player_info.len(),
                                        Ordering::SeqCst,
                                    );
                                }
                                Err(e) => {
                                    println!("could not deserialize: {e:?}")
                                }
                            }
                        }
                        Err(e) => {
                            println!("could not read: {normal}.hof - {e:?}")
                        }
                    }
                }
            },
            Err(TryRecvError::Disconnected) => {
                for (handle, _) in &acccounts {
                    handle.abort();
                }
                std::process::exit(0);
            }
            Err(TryRecvError::Empty) => {
                // We can just continue
            }
        }

        while let Ok(char) = character_receiver.try_recv() {
            handle_new_char_info(char, &mut equipment, &mut player_info);
        }

        if last_pc != player_info.len() || equipment.len() != last_ec {
            update_best_players(
                &equipment, &target, &player_info, max_level, &output,
                &acccounts,
            );
            last_ec = equipment.len();
            last_pc = player_info.len();
        } else {
            let c = CONTEXT.get().unwrap();
            c.request_repaint();
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn update_best_players(
    equipment: &HashMap<EquipmentIdent, HashSet<u32>>,
    target: &ScrapBook,
    player_info: &HashMap<u32, CharacterInfo>,
    max_level: u16,
    output: &Sender<ObserverInfo>,
    acccounts: &Vec<(JoinHandle<()>, UnboundedSender<CrawlerCommand>)>,
) {
    let per_player_counts: HashMap<_, _> = equipment
        .iter()
        .filter(|(eq, _)| !target.items.contains(eq) && eq.model_id < 100)
        .flat_map(|(_, player)| player)
        .fold(HashMap::default(), |mut acc, &p| {
            *acc.entry(p).or_insert(0) += 1;
            acc
        });

    let mut counts = [(); 10].map(|_| vec![]);
    for (player, count) in per_player_counts {
        counts[count - 1].push(player);
    }

    let mut best_players = Vec::new();
    for (count, player) in counts.into_iter().enumerate().rev() {
        best_players.extend(
            player
                .iter()
                .flat_map(|a| player_info.get(a))
                .filter(|a| a.level <= max_level)
                .map(|a| (count + 1, a.to_owned())),
        );
        if best_players.len() >= 100 {
            break;
        }
    }
    best_players.sort_by(|a, b| b.cmp(a));
    best_players.truncate(100);

    if output.send(ObserverInfo { best_players }).is_err() {
        for (handle, _) in acccounts {
            handle.abort();
        }
        std::process::exit(0);
    } else {
        let c = CONTEXT.get().unwrap();
        c.request_repaint();
    }
}

fn handle_new_char_info(
    char: CharacterInfo,
    equipment: &mut HashMap<EquipmentIdent, HashSet<u32>>,
    player_info: &mut HashMap<u32, CharacterInfo>,
) {
    for eq in char.equipment.clone() {
        equipment
            .entry(eq)
            .and_modify(|a| {
                a.insert(char.uid);
            })
            .or_insert_with(|| HashSet::from_iter([char.uid].into_iter()));
    }
    player_info.insert(char.uid, char);
}

enum CrawlerCommand {
    Pause,
    Start,
}

static PAGE_POS: AtomicUsize = AtomicUsize::new(0);
static TOTAL_PLAYERS: AtomicUsize = AtomicUsize::new(0);
static FETCHED_PLAYERS: AtomicUsize = AtomicUsize::new(0);

async fn crawl(
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<CrawlerCommand>,
    out: tokio::sync::mpsc::UnboundedSender<CharacterInfo>,
    mut started: bool,
    pages: Vec<usize>,
    server: ServerConnection,
    username: String,
) {
    let password = username.chars().rev().collect::<String>();
    let (mut session, response) = match CharacterSession::register(
        &username,
        &password,
        server.clone(),
        Gender::Male,
        Race::DarkElf,
        Class::Mage,
    )
    .await
    {
        Ok(x) => x,
        Err(_) => {
            let mut session =
                CharacterSession::new(&username, &password, server);
            let resp = session.login().await.unwrap();
            (session, resp)
        }
    };

    let mut gs = GameState::new(response).unwrap();

    let mut todo_accounts: Vec<String> = Vec::new();

    loop {
        while let Some(todo) = todo_accounts.pop() {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let r = session
                .send_command(&Command::ViewPlayer {
                    ident: todo.clone(),
                })
                .await;

            FETCHED_PLAYERS.fetch_add(1, Ordering::SeqCst);
            if let Ok(resp) = r {
                gs.update(resp).unwrap();
                let Some(player) = gs.other_players.lookup_name(&todo).cloned()
                else {
                    continue;
                };
                let equipment = player
                    .equipment
                    .0
                    .iter()
                    .flatten()
                    .filter_map(|a| a.equipment_ident())
                    .collect();

                out.send(CharacterInfo {
                    equipment,
                    name: player.name,
                    uid: player.player_id,
                    level: player.level,
                })
                .unwrap();
            }
        }

        match receiver.try_recv() {
            Ok(command) => match command {
                CrawlerCommand::Pause => {
                    started = false;
                }
                CrawlerCommand::Start => {
                    started = true;
                }
            },
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return;
            }
        }
        if started {
            // gs.other_players.reset_lookups();
            let pos =
                PAGE_POS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            let Some(page) = pages.get(pos).copied() else {
                // We fetched the entire HoF
                return;
            };

            let Ok(resp) = session
                .send_command(&Command::HallOfFamePage { page })
                .await
            else {
                continue;
            };

            gs.update(resp).unwrap();

            for hof in &gs.other_players.hall_of_fame {
                todo_accounts.push(hof.name.to_string())
            }
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn handle_player(
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
