use std::collections::HashSet;

use chrono::{DateTime, Local};
use iced::{
    alignment::Horizontal,
    theme,
    widget::{
        self, button, checkbox, column, container, horizontal_space, pick_list,
        progress_bar, row, text, Button,
    },
    Alignment, Element, Length,
};
use iced_aw::{number_input, widgets::DropDown};
use num_format::ToFormattedString;
use options::view_options;

use self::{scrapbook::view_scrapbook, underworld::view_underworld};
use crate::{
    config::{AvailableTheme, Config},
    crawler::CrawlingOrder,
    get_server_code,
    message::Message,
    player::{AccountInfo, AccountStatus},
    server::{CrawlingStatus, ServerInfo},
    top_bar, AccountIdent, AccountPage, ActionSelection, Helper, View,
};

mod options;
mod scrapbook;
pub mod underworld;

impl Helper {
    pub fn view_current_page(&self) -> Element<Message> {
        let view: Element<Message> = match &self.current_view {
            View::Account { ident, page } => self.view_account(*ident, *page),
            View::Login => self
                .login_state
                .view(&self.config.accounts, self.has_accounts()),
            View::Overview { selected, action } => {
                self.view_overview(selected, action)
            }
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
            selection(AccountPage::Options),
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
                view_scrapbook(server, player, &self.config, &self.class_images)
            }
            AccountPage::Underworld => view_underworld(
                server, player, &self.config, &self.class_images,
            ),
            AccountPage::Options => view_options(player, server, &self.config),
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

        let show_class_icons =
            checkbox("Show class icons", self.config.show_class_icons)
                .on_toggle(Message::ShowClasses);

        let max_threads =
            number_input(self.config.max_threads, 50, Message::SetMaxThreads);

        let max_threads = row!("Max threads:", horizontal_space(), max_threads)
            .width(Length::Fill)
            .align_items(Alignment::Center);

        let start_threads = number_input(
            self.config.start_threads,
            50.min(self.config.max_threads),
            Message::SetStartThreads,
        );

        let start_threads =
            row!("Starting threads:", horizontal_space(), start_threads)
                .width(Length::Fill)
                .align_items(Alignment::Center);

        let blacklist_threshold = number_input(
            self.config.blacklist_threshold,
            10,
            Message::SetBlacklistThr,
        );

        let blacklist_threshold = row!(
            "Blacklist threshhold:",
            horizontal_space(),
            blacklist_threshold
        )
        .width(Length::Fill)
        .align_items(Alignment::Center);

        let settings_column = column!(
            theme_row, auto_fetch_hof, auto_poll, max_threads, start_threads,
            blacklist_threshold, crawling_restrict, show_class_icons
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

    fn view_overview(
        &self,
        selected: &HashSet<AccountIdent>,
        currrent_action: &Option<ActionSelection>,
    ) -> Element<Message> {
        let top_bar =
            top_bar(text("Overview").size(20).into(), Some(Message::ViewLogin));

        let mut accounts = column!()
            .padding(20)
            .spacing(5)
            .width(Length::Fill)
            .align_items(Alignment::Center);

        let info_row = row!(
            center(text("Status").width(ACC_STATUS_WIDTH)),
            center(text("Server").width(SERVER_CODE_WIDTH)),
            text("Name").width(ACC_NAME_WIDTH),
            horizontal_space(),
            center(text("Underworld").width(UNDERWORLD_WIDTH)),
            center(text("Arena").width(NEXT_FIGHT_WIDTH)),
            center(text("Scrapbook").width(SCRAPBOOK_COUNT_WIDTH)),
            text("Crawling").width(CRAWLING_STATUS_WIDTH),
        )
        .spacing(10.0)
        .width(Length::Fill)
        .padding(5.0);

        let all_active: Vec<_> = self
            .servers
            .0
            .values()
            .flat_map(|a| a.accounts.values())
            .map(|a| a.ident)
            .collect();

        let cb = checkbox("", all_active.iter().all(|a| selected.contains(a)))
            .on_toggle(move |nv| Message::SetOverviewSelected {
                ident: all_active.clone(),
                val: nv,
            })
            .size(13.0);

        let this_action = Some(ActionSelection::Multi);
        let is_acting = currrent_action == &this_action;

        let mut action_button = button(
            iced_aw::core::icons::bootstrap::icon_to_text(
                iced_aw::Bootstrap::ThreeDotsVertical,
            )
            .size(18.0),
        )
        .padding(4.0);

        if is_acting {
            action_button = action_button.on_press(Message::SetAction(None))
        } else if !selected.is_empty() {
            action_button =
                action_button.on_press(Message::SetAction(this_action))
        }

        let action_dd =
            DropDown::new(action_button, self.overview_actions(), is_acting)
                .width(Length::Fill)
                .on_dismiss(Message::SetAction(None))
                .alignment(iced_aw::drop_down::Alignment::BottomStart);

        let full_row =
            row!(cb, info_row, action_dd).align_items(Alignment::Center);

        accounts = accounts.push(full_row);

        let mut servers: Vec<_> = self.servers.0.values().collect();
        servers.sort_by_key(|a| &a.ident.ident);
        for server in servers {
            let server_status: Box<str> = match &server.crawling {
                CrawlingStatus::Waiting => "Waiting".into(),
                CrawlingStatus::Restoring => "Restoring".into(),
                CrawlingStatus::CrawlingFailed(_) => "Error".into(),
                CrawlingStatus::Crawling { que, .. } => {
                    let lock = que.lock().unwrap();
                    let remaining = lock.count_remaining();
                    drop(lock);
                    if remaining == 0 {
                        "Finished".into()
                    } else {
                        remaining
                            .to_formatted_string(&self.config.num_format)
                            .into()
                    }
                }
            };

            let mut accs: Vec<_> = server.accounts.values().collect();
            accs.sort_by_key(|a| &a.name);
            for acc in accs {
                let info_row =
                    overview_row(acc, server, &server_status, &self.config);
                let selected = selected.contains(&acc.ident);

                let ident = acc.ident;

                let cb = checkbox("", selected)
                    .on_toggle(move |nv| Message::SetOverviewSelected {
                        ident: vec![ident],
                        val: nv,
                    })
                    .size(13.0);

                let this_action = Some(ActionSelection::Character(ident));
                let is_acting = currrent_action == &this_action;

                let action_button = button(
                    iced_aw::core::icons::bootstrap::icon_to_text(
                        iced_aw::Bootstrap::ThreeDotsVertical,
                    )
                    .size(18.0),
                )
                .on_press(if is_acting {
                    Message::SetAction(None)
                } else {
                    Message::SetAction(this_action)
                })
                .padding(4.0);

                let action_dd = DropDown::new(
                    action_button,
                    self.overview_actions(),
                    is_acting,
                )
                .width(Length::Fill)
                .on_dismiss(Message::SetAction(None))
                .alignment(iced_aw::drop_down::Alignment::BottomStart);

                let full_row = row!(cb, info_row, action_dd)
                    .spacing(5.0)
                    .align_items(Alignment::Center);

                accounts = accounts.push(full_row);
            }
        }

        column!(top_bar, widget::scrollable(accounts))
            .spacing(5)
            .height(Length::Fill)
            .width(Length::Fill)
            .align_items(Alignment::Center)
            .into()
    }
    fn overview_actions(&self) -> Element<Message> {
        let mut all_actions = column!().spacing(4.0);

        fn action(button: Button<Message>) -> Button<Message> {
            button.width(100.0)
        }

        all_actions = all_actions.push(action(
            button(row!(
                text("Auto Battle"),
                horizontal_space(),
                iced_aw::core::icons::bootstrap::icon_to_text(
                    iced_aw::Bootstrap::Check,
                )
            ))
            .on_press(Message::MultiAction {
                action: OverviewAction::AutoBattle(true),
            }),
        ));

        all_actions = all_actions.push(action(
            button(row!(
                text("Auto Battle"),
                horizontal_space(),
                iced_aw::core::icons::bootstrap::icon_to_text(
                    iced_aw::Bootstrap::X,
                )
            ))
            .on_press(Message::MultiAction {
                action: OverviewAction::AutoBattle(false),
            }),
        ));

        all_actions = all_actions.push(action(
            button("Logout")
                .on_press(Message::MultiAction {
                    action: OverviewAction::Logout,
                })
                .style(theme::Button::Destructive),
        ));

        all_actions.into()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OverviewAction {
    Logout,
    AutoBattle(bool),
}

const ACC_STATUS_WIDTH: f32 = 80.0;
const ACC_NAME_WIDTH: f32 = 200.0;
const SERVER_CODE_WIDTH: f32 = 50.0;
const SCRAPBOOK_COUNT_WIDTH: f32 = 60.0;
const NEXT_FIGHT_WIDTH: f32 = 60.0;
const UNDERWORLD_WIDTH: f32 = 60.0;
const CRAWLING_STATUS_WIDTH: f32 = 80.0;

fn overview_row<'a>(
    acc: &'a AccountInfo,
    server: &'a ServerInfo,
    crawling_status: &'_ str,
    config: &'a Config,
) -> Element<'a, Message> {
    let status_text = |t: &str| center(text(t).width(ACC_STATUS_WIDTH));

    let mut next_free_fight = None;

    let acc_status = match &*acc.status.lock().unwrap() {
        AccountStatus::LoggingIn => status_text("Logging in"),
        AccountStatus::Idle(_, gs) => {
            next_free_fight = Some(gs.arena.next_free_fight);
            status_text("Active")
        }
        AccountStatus::Busy(gs, reason) => {
            next_free_fight = Some(gs.arena.next_free_fight);
            status_text(reason)
        }
        AccountStatus::FatalError(_) => status_text("Error!"),
        AccountStatus::LoggingInAgain => status_text("Logging in"),
    };

    let server_code = center(
        text(get_server_code(&server.ident.url)).width(SERVER_CODE_WIDTH),
    );

    let acc_name = text(titlecase::titlecase(acc.name.as_str()).to_string())
        .width(ACC_NAME_WIDTH);

    let scrapbook_count: String = match &acc.scrapbook_info {
        Some(si) => si
            .scrapbook
            .items
            .len()
            .to_formatted_string(&config.num_format),
        None => "".into(),
    };
    let scrapbook_count = text(scrapbook_count)
        .width(SCRAPBOOK_COUNT_WIDTH)
        .horizontal_alignment(Horizontal::Center);

    let icon_to_text =
        |icon| iced_aw::core::icons::bootstrap::icon_to_text(icon).size(18.0);

    let abs = acc
        .scrapbook_info
        .as_ref()
        .map(|a| {
            if a.auto_battle {
                iced_aw::Bootstrap::LightningFill
            } else {
                iced_aw::Bootstrap::Lightning
            }
        })
        .unwrap_or(iced_aw::Bootstrap::Question);

    let next_free_fight = match next_free_fight {
        None => icon_to_text(iced_aw::Bootstrap::Question),
        Some(Some(x)) if x >= Local::now() => text(remaining_minutes(x)),
        Some(_) => icon_to_text(iced_aw::Bootstrap::Check),
    };

    let next_free_fight = row!(
        center(next_free_fight.width(25.0)),
        center(icon_to_text(abs))
    )
    .align_items(Alignment::Center)
    .spacing(4.0);

    let next_free_fight = column!(next_free_fight)
        .align_items(Alignment::Center)
        .width(NEXT_FIGHT_WIDTH);

    let underworld_info: Element<Message> = acc
        .underworld_info
        .as_ref()
        .map(|a| {
            let auto_status = if a.auto_lure {
                iced_aw::Bootstrap::LightningFill
            } else {
                iced_aw::Bootstrap::Lightning
            };

            let remaining = 5u16.saturating_sub(a.underworld.lured_today);
            let remaining = if remaining == 0 {
                icon_to_text(iced_aw::Bootstrap::Check)
            } else {
                text(remaining.to_string())
            };

            let row = row!(
                center(remaining.width(25.0)),
                center(icon_to_text(auto_status))
            )
            .align_items(Alignment::Center)
            .spacing(4.0);

            column!(row)
                .width(UNDERWORLD_WIDTH)
                .align_items(Alignment::Center)
                .into()
        })
        .unwrap_or(
            center(icon_to_text(iced_aw::Bootstrap::X))
                .width(UNDERWORLD_WIDTH)
                .into(),
        );

    let crawling_status = text(crawling_status).width(CRAWLING_STATUS_WIDTH);

    let info_row = row!(
        acc_status,
        server_code,
        acc_name,
        horizontal_space(),
        underworld_info,
        next_free_fight,
        scrapbook_count,
        crawling_status
    )
    .spacing(10.0)
    .align_items(Alignment::Center);

    button(info_row)
        .on_press(Message::ShowPlayer { ident: acc.ident })
        .width(Length::Fill)
        .height(Length::Shrink)
        .padding(4.0)
        .style(theme::Button::Secondary)
        .into()
}

fn remaining_minutes(time: DateTime<Local>) -> String {
    let now = Local::now();
    let secs = (time - now).num_seconds() % 60;
    let mins = (time - now).num_seconds() / 60;
    format!("{mins}:{secs:02}")
}

fn center(t: text::Text) -> text::Text {
    t.horizontal_alignment(Horizontal::Center)
}

pub fn view_crawling<'a>(
    server: &'a ServerInfo,
    config: &'a Config,
) -> Element<'a, Message> {
    let mut left_col = column!().align_items(Alignment::Center).spacing(10);

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

    left_col.into()
}
