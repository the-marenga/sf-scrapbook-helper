#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
pub mod crawler;
pub mod observer;
pub mod player;

use std::{
    hash::{Hash, Hasher},
    sync::{atomic::*, mpsc::*, Arc, Mutex},
    time::Duration,
};

use chrono::Local;
use crawler::{FETCHED_PLAYERS, PAGE_POS};
use eframe::egui::{self, CentralPanel, Context, Layout, SidePanel};
use observer::{observe, ObserverCommand, ObserverInfo, INITIAL_LOAD_FINISHED};
use once_cell::sync::OnceCell;
use player::{handle_player, PlayerCommand, PlayerInfo};
use serde::{Deserialize, Serialize};
use sf_api::{
    error::SFError,
    gamestate::{unlockables::EquipmentIdent, *},
    session::*,
    sso::SFAccount,
};
use tokio::{runtime::Runtime, task::JoinHandle};

static TOTAL_PLAYERS: AtomicUsize = AtomicUsize::new(0);

fn main() -> Result<(), eframe::Error> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Warn)
        .init();

    let rt = Runtime::new().expect("Unable to create Runtime");
    let _enter = rt.enter();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default(),
        ..Default::default()
    };
    eframe::run_native(
        "Scrapbook Helper v0.1.1",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_pixels_per_point(1.4);
            Box::new(Stage::start_page(None))
        }),
    )
}

type Possible<T> = Arc<Mutex<Option<T>>>;

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
        Possible<(Result<Response, SFError>, CharacterSession)>,
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
    },
    SSOLoggingIn(
        Possible<Result<Vec<CharacterSession>, SFError>>,
        JoinHandle<()>,
    ),
    SSODecide(Vec<CharacterSession>),
}

impl Stage {
    pub fn start_page(error: Option<String>) -> Stage {
        Stage::Login {
            name: "".to_owned(),
            password: "".to_owned(),
            server: "f1.sfgame.net".to_owned(),
            sso_name: "".to_string(),
            sso_password: "".to_string(),
            error,
        }
    }
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
                        let pw = ui
                            .add_sized(
                                ui.available_size(),
                                egui::TextEdit::singleline(password)
                                    .password(true),
                            )
                            .labelled_by(password_label.id);

                        if pw.lost_focus()
                            && pw.ctx.input(|a| a.key_down(egui::Key::Enter))
                        {
                            login_normal(
                                server, name, password, &mut new_stage, error,
                            );
                        }
                    });
                    ui.horizontal(|ui| {
                        let server_label = ui.label("Server: ");
                        let pw = ui
                            .add_sized(
                                ui.available_size(),
                                egui::TextEdit::singleline(server),
                            )
                            .labelled_by(server_label.id);
                        if pw.lost_focus()
                            && pw.ctx.input(|a| a.key_down(egui::Key::Enter))
                        {
                            login_normal(
                                server, name, password, &mut new_stage, error,
                            );
                        }
                    });
                    ui.add_space(12.0);

                    if ui.button("Login").clicked() {
                        login_normal(
                            server, name, password, &mut new_stage, error,
                        );
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
                        let pw = ui
                            .add_sized(
                                ui.available_size(),
                                egui::TextEdit::singleline(sso_password)
                                    .password(true),
                            )
                            .labelled_by(password_label.id);

                        if pw.lost_focus()
                            && pw.ctx.input(|a| a.key_down(egui::Key::Enter))
                        {
                            login_sso(sso_name, sso_password, &mut new_stage);
                        }
                    });

                    if ui.button("SSO Login").clicked() {
                        login_sso(sso_name, sso_password, &mut new_stage);
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

                        tokio::spawn(observe(
                            info_sender,
                            cmd_recv,
                            sb,
                            sc.clone(),
                            gs.other_players.total_player as usize,
                            initial_count,
                            gs.character.level,
                            player_hash,
                            server_hash,
                            server_url,
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
                                target_list: "".to_string(),
                            },
                            player_sender,
                            player_receiver: pi_recv,
                            last_player_response: None,
                            auto_battle: false,
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
            } => {
                if !INITIAL_LOAD_FINISHED.load(Ordering::SeqCst) {
                    ui.with_layout(
                        Layout::centered_and_justified(
                            egui::Direction::TopDown,
                        ),
                        |ui| {
                            ui.group(|ui| {
                                ui.set_height(ui.available_height() / 8.0);
                                ui.set_width(ui.available_width() / 1.5);

                                ui.label(
                                    "Loading backups. This might take a few \
                                     seconds. Please wait",
                                );

                                ui.spinner();
                            })
                        },
                    );
                    return;
                }

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

                        ui.add_space(10.0);

                        ui.group(|ui| {
                            egui::Grid::new("hof_grid").show(ui, |ui| {
                                ui.label("Crawl threads/accounts")
                                    .on_hover_text(
                                        "The amount of background accounts \
                                         created to fetch the HoF with",
                                    );
                                if ui
                                    .add(
                                        egui::DragValue::new(active)
                                            .clamp_range(1..=10),
                                    )
                                    .changed()
                                {
                                    sender
                                        .send(ObserverCommand::SetAccounts(
                                            *active,
                                        ))
                                        .unwrap();
                                };
                                ui.end_row();
                                ui.label("Max target level").on_hover_text(
                                    "The highest level of players, that will \
                                     be displayed. Also effects the \
                                     auto-battle targets",
                                );
                                if ui
                                    .add(
                                        egui::DragValue::new(max_level)
                                            .clamp_range(1..=800),
                                    )
                                    .changed()
                                {
                                    sender
                                        .send(ObserverCommand::SetMaxLevel(
                                            *max_level,
                                        ))
                                        .unwrap();
                                }

                                ui.end_row();
                            })
                        });

                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            if ui
                                .button("Pause Crawling")
                                .on_hover_text(
                                    "Stops all background characters from \
                                     crawling new HoF pages. Note that they \
                                     will finish their current page, so there \
                                     might be a short delay",
                                )
                                .clicked()
                            {
                                sender.send(ObserverCommand::Pause).unwrap()
                            }
                            if ui
                                .button("Start Crawling")
                                .on_hover_text(
                                    "Starts crawling the HoF with the amount \
                                     of background characters (threads) set",
                                )
                                .clicked()
                            {
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

                        ui.checkbox(auto_battle, "Auto Battle").on_hover_text(
                            "Automatically battles the best target as soon, \
                             as the 10 minute timer for free arena battles \
                             elapses.",
                        );

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
                                gs.arena.next_free_fight = Some(
                                    Local::now() + Duration::from_secs(60 * 10),
                                );
                            }
                        }

                        ui.add_space(20.0);

                        if ui
                            .button("Backup HoF")
                            .on_hover_text(
                                "Exports the current crawling progress to a \
                                 file in the current directory. This should \
                                 be used instead of refetching the HoF \
                                 multiple times for the same server",
                            )
                            .clicked()
                        {
                            sender.send(ObserverCommand::Export).unwrap();
                        }

                        if ui
                            .button("Restore HoF")
                            .on_hover_text(
                                "Loads the previously saved crawling data to \
                                 a file in the current directory",
                            )
                            .clicked()
                        {
                            sender.send(ObserverCommand::Restore).unwrap();
                        }

                        if ui
                            .button("Clear HoF")
                            .on_hover_text(
                                "Clears all data fetched from the HoF (in \
                                 case you want to start from 0)",
                            )
                            .clicked()
                        {
                            sender.send(ObserverCommand::Clear).unwrap();
                        }

                        if ui
                            .button("Export Player")
                            .on_hover_text(
                                "Exports information about the current player \
                                 into a json file in the current directory",
                            )
                            .clicked()
                        {
                            _ = std::fs::write(
                                format!("{}.player", &gs.character.name),
                                serde_json::to_string_pretty(&gs.clone())
                                    .unwrap(),
                            );
                        }
                        if ui
                            .button("Copy best targets")
                            .on_hover_text(
                                "Copy the optimal order to battle players \
                                 into the clipboard. This can then be used in \
                                 the mfbot",
                            )
                            .clicked()
                        {
                            ui.output_mut(|a| {
                                a.copied_text =
                                    last_response.target_list.clone()
                            });
                        }
                    });
                });
                CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.set_width(ui.available_width());
                        ui.heading("Possible Targets");
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            egui::Grid::new("hof_grid")
                                .num_columns(4)
                                .striped(true)
                                .spacing((20.0, 10.0))
                                .min_col_width(20.0)
                                .show(ui, |ui| {
                                    ui.label("Fight");
                                    ui.label("Missing");
                                    ui.label("Level");
                                    // No chance in hell, this is how you are
                                    // supposed to do this
                                    ui.label(format!(
                                        "Name{}",
                                        vec![' '; 10_000]
                                            .into_iter()
                                            .collect::<String>()
                                    ));
                                    ui.end_row();
                                    for (count, info) in
                                        &last_response.best_players
                                    {
                                        if ui.button("Fight").clicked() {
                                            player_sender
                                                .send(PlayerCommand::Attack {
                                                    name: info.name.clone(),
                                                    uid: info.uid,
                                                    mush: true,
                                                })
                                                .unwrap();
                                        }
                                        ui.label(count.to_string());
                                        ui.label(info.level.to_string());
                                        ui.label(&info.name);
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

fn login_sso(
    sso_name: &mut String,
    sso_password: &mut String,
    new_stage: &mut Option<Stage>,
) {
    let arc = Arc::new(Mutex::new(None));
    let output = arc.clone();

    let username = sso_name.clone();
    let password = sso_password.clone();

    let handle = tokio::spawn(async move {
        let account = match SFAccount::login(username, password).await {
            Ok(account) => account,
            Err(err) => {
                *output.lock().unwrap() = Some(Err(err));
                return;
            }
        };

        match account.characters().await {
            Ok(character) => {
                let vec = character.into_iter().flatten().collect::<Vec<_>>();
                *output.lock().unwrap() = Some(Ok(vec));
            }
            Err(err) => {
                *output.lock().unwrap() = Some(Err(err));
            }
        };
        let c = CONTEXT.get().unwrap();
        c.request_repaint();
    });

    *new_stage = Some(Stage::SSOLoggingIn(arc, handle));
}

fn login_normal(
    server: &mut String,
    name: &mut String,
    password: &mut String,
    new_stage: &mut Option<Stage>,
    error: &mut Option<String>,
) {
    let Some(sc) = ServerConnection::new(server) else {
        *error = Some("Invalid Server URL".to_string());
        return;
    };

    let session =
        sf_api::session::CharacterSession::new(name, password, sc.clone());

    let arc = Arc::new(Mutex::new(None));
    let arc2 = arc.clone();

    let handle = tokio::spawn(async move {
        let mut session = session;
        let res = session.login().await;
        *arc2.lock().unwrap() = Some((res, session));
        let c = CONTEXT.get().unwrap();
        c.request_repaint();
    });
    *new_stage = Some(Stage::LoggingIn(arc, handle, sc));
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct CharacterInfo {
    equipment: Vec<EquipmentIdent>,
    name: String,
    uid: u32,
    level: u16,
}
