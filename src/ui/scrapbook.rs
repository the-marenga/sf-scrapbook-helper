use ahash::HashSet;
use chrono::Local;
use iced::{
    alignment::Horizontal,
    theme,
    widget::{
        button, checkbox, column, horizontal_space, pick_list, progress_bar,
        row, scrollable, text, vertical_space, Image,
    },
    Alignment, Element, Length,
};
use iced_aw::number_input;
use num_format::ToFormattedString;

use crate::{
    config::Config,
    crawler::CrawlingOrder,
    message::Message,
    player::{AccountInfo, AccountStatus},
    server::{CrawlingStatus, ServerInfo},
    ClassImages,
};

pub fn view_scrapbook<'a>(
    server: &'a ServerInfo,
    player: &'a AccountInfo,
    config: &'a Config,
    images: &'a ClassImages,
) -> Element<'a, Message> {
    let lock = player.status.lock().unwrap();

    let gs = match &*lock {
        AccountStatus::LoggingIn => return text("Logging in").size(20).into(),
        AccountStatus::Idle(_, gs) => gs,
        AccountStatus::Busy(gs, _) => gs,
        AccountStatus::FatalError(err) => {
            return text(format!("Error: {err}")).size(20).into()
        }
        AccountStatus::LoggingInAgain => {
            return text("Logging in again".to_string()).size(20).into()
        }
    };

    let Some(si) = &player.scrapbook_info else {
        return text("Player does not have a scrapbook").size(20).into();
    };

    let mut left_col = column!().align_items(Alignment::Center).spacing(10);

    left_col = left_col.push(row!(
        text("Mushrooms:").width(Length::FillPortion(1)),
        text(gs.character.mushrooms)
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Right)
    ));

    left_col = left_col.push(row!(
        text("Items Found:").width(Length::FillPortion(1)),
        text(
            si.scrapbook
                .items
                .len()
                .to_formatted_string(&config.num_format)
        )
        .width(Length::FillPortion(1))
        .horizontal_alignment(Horizontal::Right)
    ));

    left_col = left_col.push(row!(
        text("Total Attributes:").width(Length::FillPortion(1)),
        text(
            (gs.character.attribute_basis.as_array().iter().sum::<u32>()
                + gs.character
                    .attribute_additions
                    .as_array()
                    .iter()
                    .sum::<u32>())
            .to_formatted_string(&config.num_format)
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
    let max_lvl =
        number_input(si.max_level, 9999, move |nv| Message::PlayerSetMaxLvl {
            ident: aid,
            max: nv,
        })
        .style(iced_aw::NumberInputStyles::Default);

    let max_lvl = row!(text("Max Level:"), horizontal_space(), max_lvl)
        .align_items(Alignment::Center);
    left_col = left_col.push(max_lvl);

    match &gs.arena.next_free_fight {
        Some(x) if *x >= Local::now() => {
            let t = text("Next free fight:");
            let secs = (*x - Local::now()).num_seconds() % 60;
            let mins = (*x - Local::now()).num_seconds() / 60;
            let ttt = format!("{mins}:{secs:02}");
            let r = row!(
                t.width(Length::FillPortion(1)),
                text(ttt)
                    .width(Length::FillPortion(1))
                    .horizontal_alignment(Horizontal::Right)
            );
            left_col = left_col.push(r);
        }
        _ => left_col = left_col.push("Free fight possible"),
    };

    left_col = left_col.push(
        checkbox("Auto Battle", si.auto_battle)
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

    if !si.attack_log.is_empty() {
        let mut log = column!().padding(5).spacing(5);

        for (time, target, won) in si.attack_log.iter().rev() {
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
    let mut banned = HashSet::default();

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

            banned = lock.invalid_accounts.iter().cloned().collect();

            let progress_text = text(format!(
                "Fetched {}/{}",
                crawled.to_formatted_string(&config.num_format),
                total.to_formatted_string(&config.num_format)
            ));
            left_col = left_col.push(progress_text);

            let progress = progress_bar(0.0..=total as f32, crawled as f32)
                .height(Length::Fixed(10.0));
            left_col = left_col.push(progress);

            let thread_num =
                number_input(*threads, config.max_threads, move |nv| {
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

            if config.show_crawling_restrict
                || !lock.lvl_skipped_accounts.is_empty()
            {
                let old_max = lock.max_level;
                let old_min = lock.min_level;

                let set_min_lvl =
                    number_input(lock.min_level, 9999u32, move |nv| {
                        Message::CrawlerSetMinMax {
                            server: sid,
                            min: nv,
                            max: old_max,
                        }
                    });
                let thread_num =
                    row!(text("Min Lvl: "), horizontal_space(), set_min_lvl)
                        .align_items(Alignment::Center);
                left_col = left_col.push(thread_num);

                let set_min_lvl =
                    number_input(lock.max_level, 9999u32, move |nv| {
                        Message::CrawlerSetMinMax {
                            server: sid,
                            min: old_min,
                            max: nv,
                        }
                    });
                let thread_num =
                    row!(text("Max Lvl: "), horizontal_space(), set_min_lvl)
                        .align_items(Alignment::Center);
                left_col = left_col.push(thread_num);
            }

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
        text("")
            .width(Length::FillPortion(5))
            .horizontal_alignment(Horizontal::Center),
        text("Missing")
            .width(Length::FillPortion(5))
            .horizontal_alignment(Horizontal::Center),
        text("Level")
            .width(Length::FillPortion(5))
            .horizontal_alignment(Horizontal::Center),
        text("Attributes")
            .width(Length::FillPortion(5))
            .horizontal_alignment(Horizontal::Center),
        text("Name")
            .width(Length::FillPortion(15))
            .horizontal_alignment(Horizontal::Left),
    ));
    let name_bar = scrollable(name_bar);

    let mut target_list = column!().spacing(10);
    for v in &si.best {
        if banned.contains(&v.info.name) {
            continue;
        }
        let mut target_ident = row!()
            .align_items(Alignment::Start)
            .spacing(5)
            .width(Length::FillPortion(15));

        if let Some(class) = v.info.class {
            if config.show_class_icons {
                let img = Image::new(images.get_handle(class))
                    .width(Length::FillPortion(1))
                    .content_fit(iced::ContentFit::ScaleDown);
                target_ident = target_ident.push(img);
            }
        }
        target_ident = target_ident.push(
            text(&v.info.name)
                .width(Length::FillPortion(15))
                .horizontal_alignment(Horizontal::Left),
        );

        target_list = target_list.push(row!(
            column!(button("Attack").on_press(Message::PlayerAttack {
                ident: player.ident,
                target: v.to_owned()
            }))
            .align_items(Alignment::Center)
            .width(Length::FillPortion(5)),
            text(v.missing)
                .width(Length::FillPortion(5))
                .horizontal_alignment(Horizontal::Center),
            text(v.info.level)
                .width(Length::FillPortion(5))
                .horizontal_alignment(Horizontal::Center),
            text(
                v.info
                    .stats
                    .map(|a| a.to_formatted_string(&config.num_format))
                    .unwrap_or("???".to_string())
            )
            .width(Length::FillPortion(5))
            .horizontal_alignment(Horizontal::Center),
            target_ident,
        ));
    }
    let target_list = scrollable(target_list);
    let right_col = column!(name_bar, target_list).spacing(10);

    row!(
        left_col.width(Length::Fixed(200.0)),
        right_col.width(Length::Fill)
    )
    .padding(15)
    .height(Length::Fill)
    .align_items(Alignment::Start)
    .into()
}
