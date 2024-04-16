use std::fmt::Display;

use iced::{
    theme,
    widget::{
        self, button, checkbox, column, container, horizontal_space, pick_list,
        row, text,
    },
    Alignment, Element, Length,
};
use iced_aw::number_input;
use serde::{Deserialize, Serialize};

use self::{scrapbook::view_scrapbook, underworld::view_underworld};
use crate::{
    config::AvailableTheme, get_server_code, message::Message, top_bar,
    AccountIdent, AccountPage, Helper, View,
};

mod scrapbook;
pub mod underworld;

impl Helper {
    pub fn view_current_page(&self) -> Element<Message> {
        let view: Element<Message> = match self.current_view {
            View::Account { ident, page } => self.view_account(ident, page),
            View::Login => self
                .login_state
                .view(&self.config.accounts, self.has_accounts()),
            View::Overview => self.view_overview(),
            View::Settings => self.view_settings(),
        };
        let main_part = container(view).width(Length::Fill).center_x();
        let mut res = column!();

        if self.should_update {
            let dl_button =  button("Download").on_press(
                Message::OpenLink("https://github.com/the-marenga/sf-scrapbook-helper/releases/latest".to_string())
            );

            let ignore_button = button("Ignore")
                .on_press(Message::UpdateResult(false))
                .style(theme::Button::Destructive);

            let update_msg = row!(
                horizontal_space(),
                text("A new Version is available!").size(20),
                dl_button,
                horizontal_space(),
                ignore_button,
            )
            .align_items(Alignment::Center)
            .spacing(10)
            .width(Length::Fill)
            .padding(15);

            res = res.push(update_msg);
        }
        res.push(main_part).into()
    }

    fn view_account(
        &self,
        ident: AccountIdent,
        page: AccountPage,
    ) -> Element<Message> {
        let Some((server, player)) = self.servers.get_ident(&ident) else {
            return self
                .login_state
                .view(&self.config.accounts, self.has_accounts());
        };

        let selection = |this_page: AccountPage| -> Element<Message> {
            button(text(format!("{this_page:?}")))
                .on_press(Message::ViewSubPage {
                    player: player.ident,
                    page: this_page,
                })
                .padding(4)
                .style(if this_page == page {
                    theme::Button::Primary
                } else {
                    theme::Button::Secondary
                })
                .into()
        };

        let top = row!(
            text(titlecase::titlecase(&player.name).to_string()).size(20),
            text(get_server_code(&server.ident.url))
                .horizontal_alignment(iced::alignment::Horizontal::Right)
                .size(20),
            selection(AccountPage::Scrapbook),
            selection(AccountPage::Underworld),
            button(text("Logout"))
                .on_press(Message::RemoveAccount {
                    ident: player.ident,
                })
                .padding(4)
                .style(theme::Button::Destructive)
        )
        .spacing(15)
        .align_items(Alignment::Center);

        let top_bar = top_bar(top.into(), Some(Message::ViewOverview));

        let middle = match page {
            AccountPage::Scrapbook => {
                view_scrapbook(server, player, &self.config)
            }
            AccountPage::Underworld => view_underworld(
                server, player, self.config.max_threads, &self.config,
            ),
        };

        let col_container = container(middle).center_y();

        column!(top_bar, col_container)
            .spacing(5)
            .height(Length::Fill)
            .align_items(Alignment::Center)
            .into()
    }

    fn view_settings(&self) -> Element<Message> {
        let top_row = top_bar(
            text("Settings").size(20).into(),
            if self.has_accounts() {
                Some(Message::ViewOverview)
            } else {
                Some(Message::ViewLogin)
            },
        );
        use AvailableTheme::*;
        let all_themes = [
            Light, Dark, Dracula, Nord, SolarizedLight, SolarizedDark,
            GruvboxLight, GruvboxDark, CatppuccinLatte, CatppuccinFrappe,
            CatppuccinMacchiato, CatppuccinMocha, TokyoNight, TokyoNightStorm,
            TokyoNightLight, KanagawaWave, KanagawaDragon, KanagawaLotus,
            Moonfly, Nightfly, Oxocarbon,
        ];

        let theme_picker = pick_list(
            all_themes,
            Some(self.config.theme),
            Message::ChangeTheme,
        )
        .width(Length::Fixed(200.0));

        let theme_row =
            row!(text("Theme: ").width(Length::Fixed(100.0)), theme_picker)
                .width(Length::Fill)
                .align_items(Alignment::Center);

        let auto_fetch_hof = checkbox(
            "Fetch online HoF backup during login",
            self.config.auto_fetch_newest,
        )
        .on_toggle(Message::SetAutoFetch);

        let auto_poll =
            checkbox("Keep characters logged in", self.config.auto_poll)
                .on_toggle(Message::SetAutoPoll);

        let crawling_restrict = checkbox(
            "Show advanced crawling options",
            self.config.show_crawling_restrict,
        )
        .on_toggle(Message::AdvancedLevelRestrict);

        let max_threads =
            number_input(self.config.max_threads, 50, Message::SetMaxThreads);

        let max_threads = row!("Max threads:", horizontal_space(), max_threads)
            .width(Length::Fill)
            .align_items(Alignment::Center);

        let sort_picker = pick_list(
            [BestSort::Level, BestSort::Attributes],
            Some(self.config.default_best_sort),
            Message::ChangeDefaultSort,
        )
        .width(Length::Fixed(200.0));

        let sort_best = row!(text("Default Best Sort: "), sort_picker)
            .width(Length::Fill)
            .align_items(Alignment::Center);

        let settings_column = column!(
            theme_row, auto_fetch_hof, auto_poll, max_threads, sort_best,
            crawling_restrict
        )
        .width(Length::Fixed(300.0))
        .spacing(20);

        column!(top_row, settings_column)
            .spacing(20)
            .height(Length::Fill)
            .width(Length::Fill)
            .align_items(Alignment::Center)
            .into()
    }

    fn view_overview(&self) -> Element<Message> {
        let top_bar =
            top_bar(text("Overview").size(20).into(), Some(Message::ViewLogin));

        let mut accounts = column!()
            .padding(20)
            .spacing(10)
            .width(Length::Fixed(400.0))
            .align_items(Alignment::Center);

        for server in self.servers.0.values() {
            for acc in server.accounts.values() {
                let b = button(row!(
                    text(titlecase::titlecase(acc.name.as_str()).to_string()),
                    horizontal_space(),
                    text(get_server_code(&server.ident.url))
                ))
                .on_press(Message::ShowPlayer { ident: acc.ident })
                .width(Length::Fill);
                accounts = accounts.push(b);
            }
        }

        if self.servers.len() > 0 {
            let add_button = button(
                text("+")
                    .width(Length::Fill)
                    .horizontal_alignment(iced::alignment::Horizontal::Center),
            )
            .on_press(Message::ViewLogin)
            .style(theme::Button::Positive);
            accounts = accounts.push(add_button);
        }

        column!(top_bar, widget::scrollable(accounts))
            .spacing(50)
            .height(Length::Fill)
            .width(Length::Fill)
            .align_items(Alignment::Center)
            .into()
    }
}

#[derive(
    Debug, Default, PartialEq, Eq, Clone, Copy, Serialize, Deserialize,
)]
pub enum BestSort {
    #[default]
    Level,
    Attributes,
}

impl Display for BestSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BestSort::Level => "Level",
            BestSort::Attributes => "Attributes",
        })
    }
}
