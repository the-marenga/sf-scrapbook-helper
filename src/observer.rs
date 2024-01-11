use std::{
    sync::{
        atomic::Ordering,
        mpsc::{Receiver, Sender, TryRecvError},
    },
    time::Duration,
};

use eframe::epaint::ahash::{HashMap, HashSet};
use nohash_hasher::IntMap;
use sf_api::{
    gamestate::unlockables::{EquipmentIdent, ScrapBook},
    session::ServerConnection,
};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};

use crate::{
    crawler::{crawl, CrawlerCommand, FETCHED_PLAYERS, PAGE_POS},
    CharacterInfo, CONTEXT, TOTAL_PLAYERS,
};

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

pub struct ObserverInfo {
    pub best_players: Vec<(usize, CharacterInfo)>,
    pub target_list: String,
}

pub enum ObserverCommand {
    SetAccounts(usize),
    SetMaxLevel(u16),
    UpdateFight(u32),
    Start,
    Pause,
    Export(String),
    Restore(String),
}

#[allow(clippy::too_many_arguments)]
pub async fn observe(
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
    let mut last_tl = 0;

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

        if last_pc != player_info.len() || target.items.len() != last_tl {
            update_best_players(
                &equipment, &target, &player_info, max_level, &output,
                &acccounts,
            );
            last_tl = target.items.len();
            last_pc = player_info.len();
        } else {
            let c = CONTEXT.get().unwrap();
            c.request_repaint();
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
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
    let mut scrapbook = target.items.clone();

    let best_players =
        find_best(equipment, &scrapbook, player_info, max_level, 100);

    let mut best = best_players.first().cloned();

    let mut target_list = Vec::new();
    let mut loop_count = 0;
    while let Some((count, info)) = best {
        if count == 0 || loop_count > 300 {
            break;
        }
        loop_count += 1;
        scrapbook.extend(info.equipment);
        target_list.push(info.name);
        let best_players =
            find_best(equipment, &scrapbook, player_info, max_level, 1);
        best = best_players.into_iter().next();
    }
    let target_list = target_list.join("/");
    if output
        .send(ObserverInfo {
            best_players,
            target_list,
        })
        .is_err()
    {
        for (handle, _) in acccounts {
            handle.abort();
        }
        std::process::exit(0);
    } else {
        let c = CONTEXT.get().unwrap();
        c.request_repaint();
    }
}

fn find_best(
    equipment: &HashMap<EquipmentIdent, HashSet<u32>>,
    scrapbook: &std::collections::HashSet<EquipmentIdent>,
    player_info: &std::collections::HashMap<
        u32,
        CharacterInfo,
        eframe::epaint::ahash::RandomState,
    >,
    max_level: u16,
    max_out: usize,
) -> Vec<(usize, CharacterInfo)> {
    let per_player_counts = equipment
        .iter()
        .filter(|(eq, _)| !scrapbook.contains(eq) && eq.model_id < 100)
        .flat_map(|(_, player)| player)
        .fold(IntMap::default(), |mut acc, &p| {
            *acc.entry(p).or_insert(0) += 1;
            acc
        });

    let mut counts = [(); 10].map(|_| vec![]);
    for (player, count) in per_player_counts {
        counts[count - 1].push(player);
    }
    let mut best_players = Vec::new();
    for (count, player) in counts.iter().enumerate().rev() {
        best_players.extend(
            player
                .iter()
                .flat_map(|a| player_info.get(a))
                .filter(|a| a.level <= max_level)
                .map(|a| (count + 1, a.to_owned())),
        );
        if best_players.len() >= max_out {
            break;
        }
    }
    best_players.sort_by(|a, b| b.cmp(a));
    best_players.truncate(max_out);
    best_players
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
