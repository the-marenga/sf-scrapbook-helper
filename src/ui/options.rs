use iced::{
    widget::{checkbox, column, text},
    Alignment, Element, Length,
};

use crate::{
    config::Config, message::Message, player::AccountInfo, server::ServerInfo,
};

pub fn view_options<'a>(
    player: &'a AccountInfo,
    og_server: &'a ServerInfo,
    config: &'a Config,
) -> Element<'a, Message> {
    let config = config.get_char_conf(&player.name, og_server.ident.id);

    let Some(config) = config else {
        return text(
            "Use 'Remember me' during login to store player configurations",
        )
        .size(20)
        .into();
    };

    let mut all = column!().spacing(20).width(Length::Fixed(300.0));

    all = all.push(
        checkbox("Automatically login on startup", config.login).on_toggle(
            |nv| Message::ConfigSetAutoLogin {
                name: player.name.clone(),
                server: og_server.ident.id,
                nv,
            },
        ),
    );

    all = all.push(
        checkbox("Enable auto-battle on login", config.auto_battle).on_toggle(
            |nv| Message::ConfigSetAutoBattle {
                name: player.name.clone(),
                server: og_server.ident.id,
                nv,
            },
        ),
    );

    all = all.push(
        checkbox("Enable auto-lure on login", config.auto_lure).on_toggle(
            |nv| Message::ConfigSetAutoLure {
                name: player.name.clone(),
                server: og_server.ident.id,
                nv,
            },
        ),
    );

    column!(all)
        .padding(20)
        .height(Length::Fill)
        .width(Length::Fill)
        .align_items(Alignment::Center)
        .into()
}
