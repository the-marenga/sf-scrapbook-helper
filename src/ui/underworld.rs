use iced::{
    alignment::Horizontal,
    theme,
    widget::{
        button, checkbox, column, horizontal_space, row, scrollable, text,
        vertical_space, Image,
    },
    Alignment, Element, Length,
};
use iced_aw::number_input;

use super::view_crawling;
use crate::{
    config::Config,
    message::Message,
    player::{AccountInfo, AccountStatus},
    server::ServerInfo,
    ClassImages,
};

pub fn view_underworld<'a>(
    server: &'a ServerInfo,
    player: &'a AccountInfo,
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
    left_col = left_col.push(
        checkbox("Auto Lure", info.auto_lure)
            .on_toggle(|a| Message::AutoLure {
                ident: player.ident,
                state: a,
            })
            .size(20),
    );
    left_col = left_col.push(button("Copy Targets").on_press(
        Message::CopyBestLures {
            ident: player.ident,
        },
    ));

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
    left_col = left_col.push(view_crawling(server, config));

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

#[derive(Debug, Clone)]
pub struct LureTarget {
    pub uid: u32,
    pub name: String,
}
