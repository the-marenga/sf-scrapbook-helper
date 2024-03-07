use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use async_compression::tokio::write::ZlibEncoder;
use chrono::{DateTime, Local, Utc};
use nohash_hasher::IntMap;
use serde::{Deserialize, Serialize};
use sf_api::gamestate::unlockables::EquipmentIdent;
use tokio::{
    io::{AsyncWriteExt, BufReader},
    task::yield_now,
};

use crate::{
    handle_new_char_info, CharacterInfo, CrawlingOrder, CrawlingStatus, QueID,
    WorkerQue,
};

pub async fn restore_backup(
    backup: Option<Box<ZHofBackup>>,
    total_pages: usize,
) -> RestoreData {
    let new_info = match backup {
        Some(backup) => backup,
        None => Box::new(ZHofBackup {
            todo_pages: (0..total_pages).collect(),
            invalid_pages: vec![],
            todo_accounts: vec![],
            invalid_accounts: vec![],
            order: CrawlingOrder::Random,
            export_time: None,
            characters: vec![],
        }),
    };

    let que_id = QueID::new();
    let mut todo_pages = new_info.todo_pages;
    let invalid_pages = new_info.invalid_pages;
    let todo_accounts = new_info.todo_accounts;
    let invalid_accounts = new_info.invalid_accounts;
    let order = new_info.order;

    order.apply_order(&mut todo_pages);

    let mut equipment = Default::default();
    let mut player_info = Default::default();

    for (idx, char) in new_info.characters.into_iter().enumerate() {
        if idx % 10_001 == 10_000 {
            // This loop can take a few seconds, so we make sure this does
            // not block the ui by yielding after a bit
            yield_now().await;
        }
        handle_new_char_info(char, &mut equipment, &mut player_info);
    }

    RestoreData {
        que_id,
        player_info,
        equipment,
        todo_pages,
        invalid_pages,
        todo_accounts,
        invalid_accounts,
        order,
    }
}

#[derive(Debug, Clone)]
pub struct RestoreData {
    pub que_id: QueID,
    pub player_info: IntMap<u32, CharacterInfo>,
    pub equipment: HashMap<
        EquipmentIdent,
        HashSet<u32, ahash::RandomState>,
        ahash::RandomState,
    >,
    pub todo_pages: Vec<usize>,
    pub invalid_pages: Vec<usize>,
    pub todo_accounts: Vec<String>,
    pub invalid_accounts: Vec<String>,
    pub order: CrawlingOrder,
}

impl RestoreData {
    pub fn into_status(self) -> CrawlingStatus {
        CrawlingStatus::Crawling {
            que_id: self.que_id,
            threads: 0,
            que: Arc::new(Mutex::new(WorkerQue {
                que_id: self.que_id,
                todo_pages: self.todo_pages,
                invalid_pages: self.invalid_pages,
                todo_accounts: self.todo_accounts,
                invalid_accounts: self.invalid_accounts,
                order: self.order,
                in_flight_pages: vec![],
                in_flight_accounts: vec![],
            })),
            player_info: self.player_info,
            equipment: self.equipment,
            last_update: Local::now(),
            crawling_session: None,
            recent_failures: vec![],
        }
    }
}

pub async fn get_newest_backup(
    server_ident: String,
    fetch_online: bool,
) -> Option<Box<ZHofBackup>> {
    let mut backup = ZHofBackup::read(&server_ident).await;
    if !fetch_online {
        return backup.ok().map(Box::new);
    }
    let online_time = fetch_online_hof_date(&server_ident).await;
    // Figure out, if the online version is newer, than the local backup
    let fetch_online = match (
        online_time.ok(),
        backup.as_ref().ok().and_then(|a| a.export_time),
    ) {
        (Some(ot), Some(bt)) => {
            let bt = bt.to_rfc2822();
            let bt = DateTime::parse_from_rfc2822(&bt).unwrap().to_utc();
            bt < ot
        }
        (Some(_), None) => true,
        (None, _) => false,
    };
    // If the online backup is newer, we fetch it and restore it
    if fetch_online && fetch_online_hof(&server_ident).await.is_ok() {
        println!("Fetching online Backup");
        backup = ZHofBackup::read(&server_ident).await;
    }
    backup.ok().map(Box::new)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ZHofBackup {
    #[serde(default)]
    pub todo_pages: Vec<usize>,
    #[serde(default)]
    pub invalid_pages: Vec<usize>,
    #[serde(default)]
    pub todo_accounts: Vec<String>,
    #[serde(default)]
    pub invalid_accounts: Vec<String>,
    #[serde(default)]
    pub order: CrawlingOrder,
    pub export_time: Option<DateTime<Utc>>,
    pub characters: Vec<CharacterInfo>,
}

impl ZHofBackup {
    pub async fn write(&mut self, ident: &str) -> Result<(), std::io::Error> {
        for char in &mut self.characters {
            char.fetch_date = None;
            char.stats = None;
        }
        let serialized = serde_json::to_string(&self).unwrap();
        let file = tokio::fs::File::create(format!("{}.zhof", ident)).await?;
        let mut encoder = ZlibEncoder::new(file);
        encoder.write_all(serialized.as_bytes()).await?;
        encoder.flush().await
    }

    pub async fn read(ident: &str) -> Result<ZHofBackup, std::io::Error> {
        let file = tokio::fs::File::open(format!("{}.zhof", ident)).await?;
        let reader = BufReader::new(file);
        let mut decoder =
            async_compression::tokio::bufread::ZlibDecoder::new(reader);
        let mut buffer = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut decoder, &mut buffer).await?;
        let deserialized = serde_json::from_slice(&buffer)?;
        Ok(deserialized)
    }
}

async fn fetch_online_hof_date(
    server_ident: &str,
) -> Result<DateTime<Utc>, Box<dyn std::error::Error>> {
    let resp = reqwest::get(format!(
        "https://hof-cache.marenga.dev/{server_ident}.version"
    ))
    .await?;

    match resp.error_for_status() {
        Ok(r) => {
            let text = r.text().await?;
            let date_time = DateTime::parse_from_rfc2822(&text)?;
            Ok(date_time.to_utc())
        }
        Err(e) => Err(e.into()),
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
        Err(e) => Err(e.into()),
    }
}
