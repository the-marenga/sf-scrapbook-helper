use iced::{
    alignment::Horizontal,
    widget::{column, row, scrollable, text},
    Alignment, Element, Length,
};

use crate::{
    message::Message,
    player::{AccountInfo, AccountStatus},
    server::ServerInfo,
};

pub fn view_dungeon<'a>(
    _server: &'a ServerInfo,
    player: &'a AccountInfo,
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
            return text("Logging in player again".to_string()).size(20).into()
        }
    };

    let Some(underworld) = &gs.unlocks.underworld else {
        return text("Underworld not unlocked yet".to_string())
            .size(20)
            .into();
    };

    let mut left_col = column!().align_items(Alignment::Center).spacing(10);
    left_col = left_col.push(row!(
        text("Lured Today:").width(Length::FillPortion(1)),
        text(underworld.battles_today)
            .width(Length::FillPortion(1))
            .horizontal_alignment(Horizontal::Right)
    ));

    let mut name_bar = column!();
    name_bar = name_bar
        .push(row!(
            text("Lure")
                .width(Length::FillPortion(1))
                .horizontal_alignment(Horizontal::Center),
            text("Level")
                .width(Length::FillPortion(1))
                .horizontal_alignment(Horizontal::Center),
            text("Name")
                .width(Length::FillPortion(5))
                .horizontal_alignment(Horizontal::Left),
            text("Fetched")
                .width(Length::FillPortion(1))
                .horizontal_alignment(Horizontal::Right),
        ))
        .padding(15);
    let name_bar = scrollable(name_bar);

    let target_list = column!().spacing(10);
    // for v in &player.best {
    //     target_list = target_list
    //         .push(row!(
    //             column!(button("Lure").on_press(Message::PlayerAttack {
    //                 ident: player.ident,
    //                 target: v.to_owned()
    //             }))
    //             .align_items(Alignment::Center)
    //             .width(Length::FillPortion(1)),
    //             text(v.info.level)
    //                 .width(Length::FillPortion(1))
    //                 .horizontal_alignment(Horizontal::Center),
    //             text(&v.info.name)
    //                 .width(Length::FillPortion(5))
    //                 .horizontal_alignment(Horizontal::Left),
    //             text(
    //                 &v.info
    //                     .fetch_date
    //                     .map(|a| a.format("%d-%m-%y").to_string())
    //                     .unwrap_or_else(|| { "Unknown".to_string() })
    //             )
    //             .width(Length::FillPortion(1))
    //             .horizontal_alignment(Horizontal::Right),
    //         ))
    //         .padding(15);
    // }
    let target_list = scrollable(target_list);
    let right_col = column!(name_bar, target_list).width(Length::Fill);

    row!(
        left_col.width(Length::FillPortion(1)),
        right_col.width(Length::FillPortion(3))
    )
    .padding(15)
    .height(Length::Fill)
    .align_items(Alignment::Start)
    .into()
}
