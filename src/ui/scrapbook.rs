use chrono::Local;
use iced::{
    alignment::Horizontal,
    theme,
    widget::{
        button, checkbox, column, horizontal_space, pick_list, progress_bar,
        row, scrollable, text, vertical_space,
    },
    Alignment, Element, Length,
};
use iced_aw::number_input;

use crate::{
    crawler::CrawlingOrder,
    message::Message,
    player::{AccountInfo, AccountStatus},
    server::{CrawlingStatus, ServerInfo},
};

pub fn view_scrapbook<'a>(
    server: &'a ServerInfo,
    player: &'a AccountInfo,
    max_threads: usize,
) -> Element<'a, Message> {
    let lock = player.status.lock().unwrap();
    let gs = match &*lock {
        AccountStatus::LoggingIn => return text("Loggin in").size(20).into(),
        AccountStatus::Idle(_, gs) => gs,
        AccountStatus::Busy(gs) => gs,
        AccountStatus::FatalError(err) => {
            return text(format!("Error: {err}")).size(20).into()
        }
        AccountStatus::LoggingInAgain => {
            return text(format!("Logging in player again")).size(20).into()
        }
    };

    let mut left_col = column!().align_items(Alignment::Center).spacing(10);

    left_col = left_col.push(row!(
        text("Mushrooms:").width(Length::FillPortion(1)),
        text(gs.character.mushrooms)
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Right)
    ));

    left_col = left_col.push(row!(
        text("Total Attributes:").width(Length::FillPortion(1)),
        text(
            gs.character.attribute_basis.0.iter().sum::<u32>()
                + gs.character.attribute_additions.0.iter().sum::<u32>()
        )
        .width(Length::FillPortion(1))
        .horizontal_alignment(Horizontal::Right)
    ));

    left_col = left_col.push(row!(
        text("Level:").width(Length::FillPortion(1)),
        text(gs.character.level)
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Right)
    ));

    let aid = player.ident;
    let max_lvl = number_input(player.max_level, 9999, move |nv| {
        Message::PlayerSetMaxLvl {
            ident: aid,
            max: nv,
        }
    });

    let max_lvl = row!(text("Max Level:"), horizontal_space(), max_lvl)
        .align_items(Alignment::Center);
    left_col = left_col.push(max_lvl);

    match &gs.arena.next_free_fight {
        Some(x) if *x >= Local::now() => {
            let t = text("Next free fight:");
            let secs = (*x - Local::now()).num_seconds();
            let r = row!(
                t.width(Length::FillPortion(1)),
                text(format!("{secs}s"))
                    .width(Length::FillPortion(1))
                    .horizontal_alignment(Horizontal::Right)
            );
            left_col = left_col.push(r);
        }
        _ => left_col = left_col.push("Free fight possible"),
    };

    left_col = left_col.push(
        checkbox("Auto Battle", player.auto_battle)
            .on_toggle(|a| Message::AutoBattle {
                ident: player.ident,
                state: a,
            })
            .size(20),
    );

    left_col = left_col.push(button("Copy Optimal Battle Order").on_press(
        Message::CopyBattleOrder {
            ident: player.ident,
        },
    ));

    if !player.attack_log.is_empty() {
        let mut log = column!().padding(5).spacing(5);

        for (time, target, won) in &player.attack_log {
            let time = text(format!("{}", time.time().format("%H:%M")));
            let target = text(&target.info.name);
            let row = button(row!(target, horizontal_space(), time)).style(
                match won {
                    true => theme::Button::Positive,
                    false => theme::Button::Destructive,
                },
            );
            log = log.push(row.padding(5));
        }

        left_col = left_col.push(scrollable(log).height(Length::Fixed(200.0)));
    }
    left_col = left_col.push(vertical_space());

    let sid = server.ident.id;
    match &server.crawling {
        CrawlingStatus::Crawling {
            threads,
            que,
            player_info,
            ..
        } => {
            let lock = que.lock().unwrap();
            let remaining = lock.count_remaining();
            let crawled = player_info.len();
            let total = remaining + crawled;

            let progress_text = text(format!("Fetched {}/{}", crawled, total));
            left_col = left_col.push(progress_text);

            let progress = progress_bar(0.0..=total as f32, crawled as f32)
                .height(Length::Fixed(10.0));
            left_col = left_col.push(progress);

            let thread_num = number_input(*threads, max_threads, move |nv| {
                Message::CrawlerSetThreads {
                    server: sid,
                    new_count: nv,
                }
            });
            let thread_num =
                row!(text("Threads: "), horizontal_space(), thread_num)
                    .align_items(Alignment::Center);
            left_col = left_col.push(thread_num);
            let order_picker = pick_list(
                [
                    CrawlingOrder::Random,
                    CrawlingOrder::TopDown,
                    CrawlingOrder::BottomUp,
                ],
                Some(lock.order),
                |nv| Message::OrderChange {
                    server: server.ident.id,
                    new: nv,
                },
            );
            left_col = left_col.push(
                row!(
                    text("Crawling Order:").width(Length::FillPortion(1)),
                    order_picker.width(Length::FillPortion(1))
                )
                .align_items(Alignment::Center),
            );

            let clear = button("Clear HoF").on_press(Message::ClearHof(sid));
            let save = button("Save HoF").on_press(Message::SaveHoF(sid));
            left_col = left_col.push(
                column!(row!(clear, save).spacing(10))
                    .align_items(Alignment::Center),
            );

            drop(lock);
        }
        CrawlingStatus::Waiting => {
            left_col = left_col.push(text("Waiting for Player..."));
        }
        CrawlingStatus::Restoring => {
            left_col = left_col.push(text("Loading Server Data..."));
        }
        CrawlingStatus::CrawlingFailed(_) => {
            left_col = left_col.push(text("Crawling Failed"));
        }
    }

    let mut name_bar = column!();
    name_bar = name_bar.push(row!(
        text("Attack")
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Center),
        text("Missing")
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Center),
        text("Level")
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Center),
        text("Attributes")
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Center),
        text("Name")
            .width(Length::FillPortion(3))
            .horizontal_alignment(Horizontal::Left),
        text("Fetched")
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Center),
    ));
    let name_bar = scrollable(name_bar);

    let mut target_list = column!().spacing(10);
    for v in &player.best {
        target_list = target_list.push(row!(
            column!(button("Attack").on_press(Message::PlayerAttack {
                ident: player.ident,
                target: v.to_owned()
            }))
            .align_items(Alignment::Center)
            .width(Length::FillPortion(1)),
            text(v.missing)
                .width(Length::FillPortion(1))
                .horizontal_alignment(Horizontal::Center),
            text(v.info.level)
                .width(Length::FillPortion(1))
                .horizontal_alignment(Horizontal::Center),
            text(
                v.info
                    .stats
                    .map(|a| a.to_string())
                    .unwrap_or("???".to_string())
            )
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Center),
            text(&v.info.name)
                .width(Length::FillPortion(3))
                .horizontal_alignment(Horizontal::Left),
            text(
                &v.info
                    .fetch_date
                    .map(|a| a.format("%d-%m-%y").to_string())
                    .unwrap_or_else(|| { "???".to_string() })
            )
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Center),
        ));
    }
    let target_list = scrollable(target_list);
    let right_col = column!(name_bar, target_list)
        .width(Length::Fill)
        .spacing(5);

    row!(
        left_col.width(Length::FillPortion(1)),
        right_col.width(Length::FillPortion(3))
    )
    .padding(15)
    .height(Length::Fill)
    .align_items(Alignment::Start)
    .into()
}
