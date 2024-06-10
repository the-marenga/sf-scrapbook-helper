use iced::{
    alignment::Horizontal,
    theme,
    widget::{
        button, column, horizontal_space, pick_list, progress_bar, row,
        scrollable, text, vertical_space, Image,
    },
    Alignment, Element, Length,
};
use iced_aw::number_input;

use crate::{
    config::Config,
    crawler::CrawlingOrder,
    message::Message,
    player::{AccountInfo, AccountStatus},
    server::{CrawlingStatus, ServerInfo},
    ClassImages,
};

pub fn view_underworld<'a>(
    server: &'a ServerInfo,
    player: &'a AccountInfo,
    max_threads: usize,
    config: &'a Config,
    images: &'a ClassImages,
) -> Element<'a, Message> {
    let lock = player.status.lock().unwrap();
    let _gs = match &*lock {
        AccountStatus::LoggingIn => return text("Loggin in").size(20).into(),
        AccountStatus::Idle(_, gs) => gs,
        AccountStatus::Busy(gs, _) => gs,
        AccountStatus::FatalError(err) => {
            return text(format!("Error: {err}")).size(20).into()
        }
        AccountStatus::LoggingInAgain => {
            return text("Logging in player again".to_string()).size(20).into()
        }
    };

    let Some(info) = &player.underworld_info else {
        return text("Underworld not unlocked yet".to_string())
            .size(20)
            .into();
    };

    let mut left_col = column!().align_items(Alignment::Center).spacing(10);
    left_col = left_col.push(row!(
        text("Lured Today:").width(Length::FillPortion(1)),
        text(format!("{}/5", info.underworld.lured_today))
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Right),
    ));

    let souls = info.underworld.souls_current;
    let souls_limit = info.underworld.souls_limit;

    left_col = left_col.push(row!(
        text("Souls Filled:").width(Length::FillPortion(1)),
        text(format!(
            "{:.0}%",
            (souls as f32 / (souls_limit.max(1)) as f32) * 100.0
        ))
        .width(Length::FillPortion(1))
        .horizontal_alignment(Horizontal::Right),
    ));

    let avg_lvl = info
        .underworld
        .units
        .as_array()
        .iter()
        .map(|a| a.level as u64)
        .sum::<u64>() as f32
        / 3.0;
    left_col = left_col.push(row!(
        text("Avg Unit Level:").width(Length::FillPortion(1)),
        text(format!("{:.0}", avg_lvl))
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Right),
    ));
    let aid = player.ident;
    let max_lvl = number_input(info.max_level, 9999, move |nv| {
        Message::PlayerSetMaxUndergroundLvl {
            ident: aid,
            lvl: nv,
        }
    });
    let max_lvl = row!(text("Max Level:"), horizontal_space(), max_lvl)
        .align_items(Alignment::Center);
    left_col = left_col.push(max_lvl);

    if !info.attack_log.is_empty() {
        let mut log = column!().padding(5).spacing(5);

        for (time, target, won) in info.attack_log.iter().rev() {
            let time = text(format!("{}", time.time().format("%H:%M")));
            let target = text(target);
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
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Center),
        text("Level")
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Center),
        text("Items")
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Center),
        text("Name")
            .width(Length::FillPortion(3))
            .horizontal_alignment(Horizontal::Left),
    ));
    let name_bar = scrollable(name_bar);

    let mut target_list = column!().spacing(10);
    for v in &info.best {
        let mut target_ident = row!()
            .align_items(Alignment::Start)
            .spacing(5)
            .width(Length::FillPortion(3));

        if let Some(class) = v.class {
            if config.show_class_icons {
                let img = Image::new(images.get_handle(class))
                    .width(Length::FillPortion(1))
                    .content_fit(iced::ContentFit::ScaleDown);
                target_ident = target_ident.push(img);
            }
        }
        target_ident = target_ident.push(
            text(&v.name)
                .width(Length::FillPortion(15))
                .horizontal_alignment(Horizontal::Left),
        );

        target_list = target_list.push(row!(
            column!(button("Lure").on_press_maybe(
                if info.underworld.lured_today >= 5 {
                    None
                } else {
                    Some(Message::PlayerLure {
                        ident: player.ident,
                        target: LureTarget {
                            uid: v.uid,
                            name: v.name.clone(),
                        },
                    })
                }
            ))
            .align_items(Alignment::Center)
            .width(Length::FillPortion(1)),
            text(v.level)
                .width(Length::FillPortion(1))
                .horizontal_alignment(Horizontal::Center),
            text(v.equipment.len())
                .width(Length::FillPortion(1))
                .horizontal_alignment(Horizontal::Center),
            target_ident
        ));
    }
    let target_list = scrollable(target_list);
    let right_col = column!(name_bar, target_list)
        .width(Length::Fill)
        .spacing(10);

    row!(
        left_col.width(Length::FillPortion(1)),
        right_col.width(Length::FillPortion(3))
    )
    .padding(15)
    .height(Length::Fill)
    .align_items(Alignment::Start)
    .into()
}

#[derive(Debug, Clone)]
pub struct LureTarget {
    pub uid: u32,
    pub name: String,
}
