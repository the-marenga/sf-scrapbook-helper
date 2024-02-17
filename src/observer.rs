use std::{
    collections::HashSet,
    io::prelude::*,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, TryRecvError},
    },
    time::Duration,
};

use chrono::{DateTime, Utc};
use eframe::epaint::ahash::{HashMap, HashMapExt};
use flate2::{
    write::{ZlibDecoder, ZlibEncoder},
    Compression,
};
use nohash_hasher::IntMap;
use serde::{Deserialize, Serialize};
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
    Export,
    Restore,
    Clear,
}

pub static INITIAL_LOAD_FINISHED: AtomicBool = AtomicBool::new(false);

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
    server_url: String,
) -> ! {
    let server_ident = &server_url
        .trim_start_matches("https")
        .chars()
        .filter(|a| a.is_ascii_alphanumeric())
        .collect::<String>();

    let mut player_info: IntMap<u32, CharacterInfo> = Default::default();
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

    if !restore_backup(server_ident, &mut equipment, &mut player_info) {
        // We could not restore an existing backup, so we fetch one online
        match fetch_online_hof(server_ident).await {
            Ok(_) => {
                // If we managed to fetch one, we should load it
                restore_backup(server_ident, &mut equipment, &mut player_info);
            }
            Err(e) => {
                eprintln!("Could not fetch HoF: {e}")
            }
        }
    }
    update_best_players(
        &equipment, &target, &player_info, max_level, &output, &acccounts,
    );

    let mut last_tl = target.items.len();
    let mut last_pc = player_info.len();
    let mut level_changed = false;

    INITIAL_LOAD_FINISHED.store(true, Ordering::SeqCst);

    let mut caa = initial_count;

    loop {
        match receiver.try_recv() {
            Ok(data) => {
                match data {
                    ObserverCommand::UpdateFight(found) => {
                        target.items.extend(
                            player_info.get(&found).unwrap().equipment.iter(),
                        );
                    }
                    ObserverCommand::SetMaxLevel(max) => {
                        max_level = max;
                        level_changed = true;
                    }
                    ObserverCommand::SetAccounts(count) => {
                        caa = count;
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

                        for (_, sender) in &acccounts[0..count] {
                            _ = sender.send(CrawlerCommand::Start);
                        }
                        for (_, sender) in &acccounts[count..] {
                            _ = sender.send(CrawlerCommand::Pause);
                        }
                    }
                    ObserverCommand::Start => {
                        for (_, sender) in &acccounts[..caa] {
                            _ = sender.send(CrawlerCommand::Start);
                        }
                    }
                    ObserverCommand::Pause => {
                        for (_, sender) in &acccounts {
                            _ = sender.send(CrawlerCommand::Pause);
                        }
                    }
                    ObserverCommand::Export => {
                        export_backup(server_ident, &player_info);
                    }
                    ObserverCommand::Restore => {
                        restore_backup(
                            server_ident, &mut equipment, &mut player_info,
                        );
                    }
                    ObserverCommand::Clear => {
                        PAGE_POS.store(0, Ordering::SeqCst);
                        FETCHED_PLAYERS.store(0, Ordering::SeqCst);
                        equipment.clear();
                        player_info.clear();
                    }
                }
                continue;
            }
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

        if level_changed
            || last_pc != player_info.len()
            || target.items.len() != last_tl
        {
            update_best_players(
                &equipment, &target, &player_info, max_level, &output,
                &acccounts,
            );
            last_tl = target.items.len();
            last_pc = player_info.len();
            level_changed = false;
        } else {
            let c = CONTEXT.get().unwrap();
            c.request_repaint();
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn fetch_online_hof(
    server_ident: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let resp = reqwest::get(format!(
        "https://hof-cache.marenga.dev/{server_ident}.zhof"
    ))
    .await?;

    match resp.error_for_status() {
        Ok(r) => {
            let bytes = r.bytes().await?;
            tokio::fs::write(format!("{server_ident}.zhof"), bytes).await?;
            Ok(())
        }
        Err(e) => return Err(e.into()),
    }
}

fn export_backup(server_ident: &str, player_info: &IntMap<u32, CharacterInfo>) {
    let str = HofBackup {
        current_page: PAGE_POS.load(Ordering::SeqCst),
        characters: player_info.iter().map(|a| a.1.clone()).collect::<Vec<_>>(),
        export_time: Some(Utc::now()),
    };

    let str = serde_json::to_string(&str).unwrap();

    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(str.as_bytes()).unwrap();
    let compressed_bytes = e.finish().unwrap();
    _ = std::fs::write(&format!("{server_ident}.zhof"), &compressed_bytes);
}

#[derive(Debug, Serialize, Deserialize)]
struct HofBackup {
    current_page: usize,
    characters: Vec<CharacterInfo>,
    export_time: Option<DateTime<Utc>>,
}

fn restore_backup(
    server_ident: &str,
    equipment: &mut HashMap<EquipmentIdent, HashSet<u32>>,
    player_info: &mut IntMap<u32, CharacterInfo>,
) -> bool {
    let options = [true, false];

    for is_compressed in options {
        let file_name = format!(
            "{server_ident}.{}hof",
            if is_compressed { "z" } else { "" }
        );

        let mut file = match std::fs::read(&file_name) {
            Ok(t) => t,
            Err(_) => {
                continue;
            }
        };

        let uncompressed = match is_compressed {
            true => {
                let mut decoder = ZlibDecoder::new(Vec::new());
                if decoder.write_all(&mut file).is_err() {
                    eprintln!("Could not decode archive");
                    continue;
                }
                let Ok(decoded) = decoder.finish() else {
                    eprintln!("Could not finish decoding archive");
                    continue;
                };
                decoded
            }
            false => file,
        };

        let Ok(str) = String::from_utf8(uncompressed) else {
            eprintln!("data is not utf8");
            continue;
        };

        let backup = match serde_json::from_str::<HofBackup>(&str) {
            Ok(x) => x,
            _ => {
                match serde_json::from_str::<(usize, Vec<CharacterInfo>)>(&str)
                {
                    Ok(x) => HofBackup {
                        current_page: x.0,
                        characters: x.1,
                        export_time: None,
                    },
                    Err(_) => {
                        continue;
                    }
                }
            }
        };

        for char in backup.characters {
            handle_new_char_info(char, equipment, player_info);
        }
        PAGE_POS.store(backup.current_page, Ordering::SeqCst);
        FETCHED_PLAYERS.store(player_info.len(), Ordering::SeqCst);
        return true;
    }
    false
}

fn update_best_players(
    equipment: &HashMap<EquipmentIdent, HashSet<u32>>,
    target: &ScrapBook,
    player_info: &IntMap<u32, CharacterInfo>,
    max_level: u16,
    output: &Sender<ObserverInfo>,
    acccounts: &Vec<(JoinHandle<()>, UnboundedSender<CrawlerCommand>)>,
) {
    let mut scrapbook = target.items.clone();
    let mut per_player_counts = IntMap::with_capacity(player_info.len());
    for (eq, players) in equipment {
        if scrapbook.contains(eq) || eq.model_id >= 100 {
            continue;
        }
        for player in players {
            *per_player_counts.entry(*player).or_insert(0) += 1;
        }
    }

    per_player_counts.retain(|a, _| {
        let Some(info) = player_info.get(a) else {
            return false;
        };
        if info.level > max_level {
            return false;
        }
        true
    });

    let best_players = find_best(&per_player_counts, player_info, 100);

    let mut best = best_players.first().cloned();

    let mut target_list = Vec::new();
    let mut loop_count = 0;
    while let Some((new_count, info)) = best {
        if loop_count > 300 || new_count <= 1 {
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
            // We decrease the new equipment count of all players, that have
            // the same item as the one we just "found"
            for player in players {
                let ppc = per_player_counts.entry(*player).or_insert(1);
                *ppc = ppc.saturating_sub(1);
            }
        }

        scrapbook.extend(info.equipment);
        target_list.push(info.name);
        let best_players = find_best(&per_player_counts, player_info, 1);
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
    }
    let c = CONTEXT.get().unwrap();
    c.request_repaint();
}

fn find_best(
    per_player_counts: &IntMap<u32, usize>,
    player_info: &IntMap<u32, CharacterInfo>,
    max_out: usize,
) -> Vec<(usize, CharacterInfo)> {
    // Prune the counts to make computation faster
    let mut max = 1;
    let mut counts = [(); 11].map(|_| vec![]);
    for (player, count) in per_player_counts.iter().map(|a| (*a.0, *a.1)) {
        if count < max {
            continue;
        }
        max = max.max(count);
        counts[(count - 1).clamp(0, 10)].push(player);
    }

    let mut best_players = Vec::new();
    for (count, players) in counts.iter().enumerate().rev() {
        best_players.extend(
            players
                .iter()
                .flat_map(|a| player_info.get(a))
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
    player_info: &mut IntMap<u32, CharacterInfo>,
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
