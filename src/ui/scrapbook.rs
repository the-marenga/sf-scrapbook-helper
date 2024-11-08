use std::fmt::Write;

use chrono::Local;
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
use num_format::ToFormattedString;

use super::{remaining_minutes, view_crawling};
use crate::{
    config::Config,
    message::Message,
    player::{AccountInfo, AccountStatus},
    server::ServerInfo,
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

    let max_attributes = number_input(si.max_attributes, 9_999_999, move |nv| {
        Message::PlayerSetMaxAttributes {
            ident: aid,
            max: nv,
        }
    })
    .style(iced_aw::NumberInputStyles::Default);

    let max_attributes =
        row!(text("Max Attributes:"), horizontal_space(), max_attributes)
            .align_items(Alignment::Center);
    left_col = left_col.push(max_attributes);

    match &gs.arena.next_free_fight {
        Some(x) if *x >= Local::now() => {
            let t = text("Next free fight:");
            let r = row!(
                t.width(Length::FillPortion(1)),
                text(remaining_minutes(*x))
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
            let mut info = target.info.name.to_string();
            if *won {
                _ = info.write_fmt(format_args!(" (+{})", target.missing));
            }
            let target = text(&info);
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
