use std::{
    sync::{atomic::AtomicU64, Arc, Mutex},
    time::Duration,
};

use iced::{
    theme,
    widget::{
        self, button, checkbox, column, container, horizontal_space, row, text,
        text_input,
    },
    Alignment, Command, Element, Length, Renderer, Theme,
};
use sf_api::{
    error::SFError,
    gamestate::GameState,
    session::{PWHash, ServerConnection, Session},
    sso::{SFAccount, SSOAuth, SSOProvider},
};
use tokio::time::sleep;

use crate::{
    config::AccountConfig, get_server_code, message::Message, top_bar,
    AccountID, AccountIdent, AccountInfo, AccountPage, Helper, ServerIdent,
    View,
};

pub struct LoginState {
    pub login_typ: LoginType,
    pub name: String,
    pub password: String,
    pub server: String,
    pub remember_me: bool,
    pub error: Option<String>,
    pub active_sso: Vec<SSOLogin>,
    pub import_que: Vec<Session>,
    pub google_sso: Arc<Mutex<SSOStatus>>,
    pub steam_sso: Arc<Mutex<SSOStatus>>,
}

pub enum SSOStatus {
    Waiting { url: String },
    Initializing,
}

#[derive(Debug)]
pub struct SSOLogin {
    pub ident: SSOIdent,
    pub status: SSOLoginStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SSOIdent {
    SF(String),
    Google(String),
    Steam(String),
}

#[derive(Debug)]
pub enum SSOLoginStatus {
    Loading,
    Success,
}

impl LoginState {
    pub fn view(
        &self,
        accounts: &[AccountConfig],
        has_active: bool,
    ) -> Element<Message> {
        let login_type_button = |label, filter, current_filter| {
            let label: widget::text::Text<'_, Theme, Renderer> = text(label);
            let button = button(label).style(if filter == current_filter {
                theme::Button::Primary
            } else {
                theme::Button::Secondary
            });
            button
                .on_press(Message::LoginViewChanged(filter))
                .padding(4)
        };

        let mut login_selection = row![
            login_type_button("Regular", LoginType::Regular, self.login_typ),
            login_type_button(
                "S&F Account",
                LoginType::SFAccount,
                self.login_typ
            ),
            login_type_button("Steam", LoginType::Steam, self.login_typ),
            login_type_button("Google", LoginType::Google, self.login_typ),
        ]
        .align_items(Alignment::Center)
        .spacing(10);

        if !accounts.is_empty() {
            login_selection = login_selection.push(login_type_button(
                "Saved",
                LoginType::Saved,
                self.login_typ,
            ))
        }

        if !self.active_sso.is_empty() {
            login_selection = login_selection.push(login_type_button(
                "SSO",
                LoginType::SSOAccounts,
                self.login_typ,
            ))
        }

        if !self.import_que.is_empty() {
            login_selection = login_selection.push(login_type_button(
                "SSO Chars",
                LoginType::SSOChars,
                self.login_typ,
            ))
        }

        let top_bar = top_bar(
            login_selection.into(),
            if has_active {
                Some(Message::ViewOverview)
            } else {
                None
            },
        );

        let current_login = match self.login_typ {
            LoginType::SFAccount => {
                let title = text("S&F Account Login").size(20);
                let name_input = text_input("Username", &self.name)
                    .on_input(Message::LoginNameInputChange);

                let pw_input = text_input("Password", &self.password)
                    .on_input(Message::LoginPWInputChange)
                    .on_submit(Message::LoginSFSubmit)
                    .secure(true);

                let sso_login_button =
                    button("Login").on_press(Message::LoginSFSubmit).padding(4);

                let remember_me = checkbox("Remember me", self.remember_me)
                    .on_toggle(Message::RememberMe);
                let options_row = row!(remember_me)
                    .width(Length::Fill)
                    .align_items(Alignment::Start);
                let error_msg = row!(
                    horizontal_space(),
                    text(
                        self.error
                            .as_ref()
                            .map(|a| format!("Error: {a}"))
                            .unwrap_or_default()
                    ),
                    horizontal_space()
                )
                .width(Length::Fill)
                .align_items(Alignment::Center);

                column![
                    title, name_input, pw_input, options_row, sso_login_button,
                    error_msg
                ]
            }
            LoginType::Regular => {
                let title: widget::text::Text<'_, Theme, Renderer> =
                    text("Login").size(20);
                let name_input = text_input("Username", &self.name)
                    .on_input(Message::LoginNameInputChange);
                let pw_input = text_input("Password", &self.password)
                    .on_input(Message::LoginPWInputChange)
                    .on_submit(Message::LoginRegularSubmit)
                    .secure(true);
                let server_input = text_input("f1.sfgame.net", &self.server)
                    .on_input(Message::LoginServerChange)
                    .on_submit(Message::LoginRegularSubmit);
                let regular_login_button = button("Login")
                    .on_press(Message::LoginRegularSubmit)
                    .padding(4);

                let remember_me = checkbox("Remember me", self.remember_me)
                    .on_toggle(Message::RememberMe);
                let options_row = row!(remember_me)
                    .width(Length::Fill)
                    .align_items(Alignment::Start);

                column![
                    title, name_input, pw_input, server_input, options_row,
                    regular_login_button
                ]
            }
            LoginType::Steam => {
                let title: widget::text::Text<'_, Theme, Renderer> =
                    text("Steam").size(20);

                let info: Element<Message> =
                    match &*self.steam_sso.lock().unwrap() {
                        SSOStatus::Waiting { url } => button(text("Login"))
                            .on_press(Message::OpenLink(url.to_string()))
                            .into(),
                        _ => text("Waiting...").into(),
                    };

                let info = container(info).padding(20);
                column!(title, info)
            }
            LoginType::Google => {
                let title: widget::text::Text<'_, Theme, Renderer> =
                    text("Google").size(20);

                let info: Element<Message> =
                    match &*self.google_sso.lock().unwrap() {
                        SSOStatus::Waiting { url } => button(text("Login"))
                            .on_press(Message::OpenLink(url.to_string()))
                            .into(),
                        _ => text("Waiting...").into(),
                    };

                let info = container(info).padding(20);
                column!(title, info)
            }
            LoginType::Saved => {
                let title: widget::text::Text<'_, Theme, Renderer> =
                    text("Accounts").size(20);

                let mut accounts_col =
                    column!().spacing(10).width(Length::Fill).padding(20);

                for acc in accounts {
                    match &acc {
                        AccountConfig::Regular { name, server, .. } => {
                            let login_msg = Message::Login {
                                account: acc.clone(),
                                auto_login: false,
                            };

                            // TODO: Make sure they can not login twice

                            let server_ident = get_server_code(server);

                            let button = button(
                                row!(
                                    text(
                                        titlecase::titlecase(name).to_string()
                                    ),
                                    horizontal_space(),
                                    text(server_ident)
                                )
                                .width(Length::Fill),
                            )
                            .on_press(login_msg)
                            .width(Length::Fill);
                            accounts_col = accounts_col.push(button);
                        }
                        AccountConfig::SF { name, .. } => {
                            let login_msg = Message::Login {
                                account: acc.clone(),
                                auto_login: false,
                            };

                            let button = button(
                                row!(
                                    text(
                                        titlecase::titlecase(name).to_string()
                                    ),
                                    horizontal_space(),
                                    text("SF")
                                )
                                .width(Length::Fill),
                            )
                            .on_press(login_msg)
                            .style(theme::Button::Positive)
                            .width(Length::Fill);
                            accounts_col = accounts_col.push(button);
                        }
                    };
                }

                let scroll = widget::scrollable(accounts_col);

                column!(title, scroll)
            }
            LoginType::SSOAccounts => {
                let title: widget::text::Text<'_, Theme, Renderer> =
                    text("SSO Accounts").size(20);

                let mut col = column!()
                    .padding(20)
                    .spacing(10)
                    .width(Length::Fixed(400.0))
                    .align_items(Alignment::Center);

                for active in &self.active_sso {
                    let button = button(
                        row!(
                            text(
                                titlecase::titlecase(match &active.ident {
                                    SSOIdent::SF(name)
                                    | SSOIdent::Google(name)
                                    | SSOIdent::Steam(name) => name.as_str(),
                                })
                                .to_string()
                            ),
                            horizontal_space(),
                            text(match active.status {
                                SSOLoginStatus::Loading => "Loading..",
                                SSOLoginStatus::Success => "",
                            })
                        )
                        .width(Length::Fill),
                    )
                    .width(Length::Fill)
                    .style(match active.status {
                        SSOLoginStatus::Loading => theme::Button::Secondary,
                        SSOLoginStatus::Success => theme::Button::Positive,
                    })
                    .on_press_maybe(match active.status {
                        SSOLoginStatus::Loading => None,
                        SSOLoginStatus::Success => {
                            Some(Message::LoginViewChanged(LoginType::SSOChars))
                        }
                    });

                    col = col.push(button);
                }
                column!(title, widget::scrollable(col))
            }
            LoginType::SSOChars => {
                let title: widget::text::Text<'_, Theme, Renderer> =
                    text("SSO Characters").size(20);

                let mut col = column!()
                    .padding(20)
                    .spacing(10)
                    .width(Length::Fixed(400.0))
                    .align_items(Alignment::Center);

                for (pos, active) in self.import_que.iter().enumerate() {
                    let ident = get_server_code(active.server_url().as_str());

                    let button = button(
                        row!(
                            text(
                                titlecase::titlecase(active.username())
                                    .to_string()
                            ),
                            horizontal_space(),
                            text(ident)
                        )
                        .width(Length::Fill),
                    )
                    .width(Length::Fill)
                    .on_press(Message::SSOImport { pos });

                    col = col.push(button);
                }
                column!(title, widget::scrollable(col))
            }
        };

        let col = current_login
            .padding(20)
            .spacing(10)
            .width(Length::Fixed(400.0))
            .height(Length::Fill)
            .align_items(Alignment::Center);

        let col_container = container(col).center_y();

        column!(top_bar, col_container)
            .height(Length::Fill)
            .align_items(Alignment::Center)
            .spacing(50)
            .into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum LoginType {
    Regular,
    SFAccount,
    Steam,
    Google,
    Saved,
    SSOAccounts,
    SSOChars,
}

pub struct SSOValidator {
    pub status: Arc<Mutex<SSOStatus>>,
    pub provider: SSOProvider,
}

impl SSOValidator {
    pub async fn check(
        &self,
    ) -> Result<Option<(Vec<Result<Session, SFError>>, String)>, SFError> {
        sleep(Duration::from_millis(fastrand::u64(500..=1000))).await;
        let mut auth = SSOAuth::new(self.provider).await?;
        {
            *self.status.lock().unwrap() = SSOStatus::Waiting {
                url: auth.auth_url().to_string(),
            }
        }

        for _ in 0..50 {
            let resp = auth.try_login().await?;
            match resp {
                sf_api::sso::AuthResponse::Success(res) => {
                    println!("Success");
                    let name = res.username().to_string();
                    let chars = res.characters().await?;
                    return Ok(Some((chars, name)));
                }
                sf_api::sso::AuthResponse::NoAuth(res) => {
                    auth = res;
                }
            }
            sleep(Duration::from_secs(6)).await;
        }
        {
            *self.status.lock().unwrap() = SSOStatus::Initializing
        }
        Ok(None)
    }
}

impl Helper {
    pub fn login_regular(
        &mut self,
        name: String,
        server: String,
        pw_hash: PWHash,
        remember: bool,
        auto_login: bool,
    ) -> Command<Message> {
        let name = name.trim().to_string();
        let server = server.trim().to_string();

        let Some(con) = ServerConnection::new(&server) else {
            self.login_state.error = Some("Invalid Server URL".to_string());
            return Command::none();
        };

        let session =
            sf_api::session::Session::new_hashed(&name, pw_hash.clone(), con);

        self.login(session, remember, PlayerAuth::Normal(pw_hash), auto_login)
    }

    pub fn login(
        &mut self,
        mut session: sf_api::session::Session,
        remember: bool,
        auth: PlayerAuth,
        auto_login: bool,
    ) -> Command<Message> {
        let server_ident = ServerIdent::new(session.server_url().as_str());
        let Some(connection) = ServerConnection::new(&server_ident.url) else {
            self.login_state.error =
                Some("Server Url is not valid".to_string());
            return Command::none();
        };
        let name: String = session
            .username()
            .chars()
            .map(|a| a.to_ascii_lowercase())
            .collect();

        let account_id = AccountID::new();
        let account_ident = AccountIdent {
            server_id: server_ident.id,
            account: account_id,
        };
        let info = AccountInfo::new(&name, auth, account_ident);
        let server = self
            .servers
            .get_or_insert_default(server_ident, connection, None);

        if let Some((_, existing)) =
            server.accounts.iter().find(|(_, a)| a.name == name)
        {
            self.current_view = View::Account {
                ident: existing.ident,
                page: AccountPage::Scrapbook,
            };
            return Command::none();
        }
        if !auto_login {
            self.current_view = View::Account {
                ident: info.ident,
                page: AccountPage::Scrapbook,
            };
        }
        server.accounts.insert(info.ident.account, info);
        static WAITING: AtomicU64 = AtomicU64::new(0);

        Command::perform(
            async move {
                // This likely has some logic issues
                let w =
                    WAITING.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if w > 0 {
                    sleep(Duration::from_secs(w)).await;
                }
                let resp = session.login().await.inspect(|_| {
                    WAITING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                })?;
                let gs = GameState::new(resp)?;
                let gs = Box::new(gs);
                Ok((gs, Box::new(session)))
            },
            move |a: Result<_, SFError>| match a {
                Ok((gs, session)) => Message::LoggininSuccess {
                    ident: account_ident,
                    gs,
                    session,
                    remember,
                },
                Err(err) => Message::LoggininFailure {
                    ident: account_ident,
                    error: err.to_string(),
                },
            },
        )
    }

    pub fn login_sf_acc(
        &mut self,
        name: String,
        pwhash: PWHash,
        remember_sf: bool,
        auto_login: bool,
    ) -> Command<Message> {
        let ident = SSOIdent::SF(name.clone());
        self.login_state.login_typ = LoginType::SSOAccounts;
        if self
            .login_state
            .active_sso
            .iter()
            .any(|a| matches!(&a.ident, SSOIdent::SF(s) if s.as_str() == name.as_str()))
        {
            return Command::none();
        }
        self.login_state.active_sso.push(SSOLogin {
            ident: ident.clone(),
            status: SSOLoginStatus::Loading,
        });

        let n2 = name.clone();
        let p2 = pwhash.clone();
        Command::perform(
            async move {
                let account = SFAccount::login_hashed(n2, p2).await?;
                account.characters().await.into_iter().flatten().collect()
            },
            move |res| match res {
                Ok(chars) => Message::SSOLoginSuccess {
                    name,
                    pass: pwhash,
                    chars,
                    remember: remember_sf,
                    auto_login,
                },
                Err(error) => Message::SSOLoginFailure {
                    name,
                    error: error.to_string(),
                },
            },
        )
    }
}

#[allow(clippy::upper_case_acronyms)]
pub enum PlayerAuth {
    Normal(PWHash),
    SSO,
}
