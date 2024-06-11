use iced::Theme;
use num_format::CustomFormat;
use serde::{Deserialize, Serialize};
use sf_api::session::PWHash;

use crate::{server::ServerIdent, ServerID};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub accounts: Vec<AccountConfig>,
    pub theme: AvailableTheme,
    pub base_name: String,
    pub auto_fetch_newest: bool,
    #[serde(default)]
    pub auto_poll: bool,
    #[serde(default = "default_threads")]
    pub max_threads: usize,
    #[serde(default)]
    pub show_crawling_restrict: bool,
    #[serde(default = "default_class_icons")]
    pub show_class_icons: bool,
    #[serde(default = "default_blacklist_threshhold")]
    pub blacklist_threshold: usize,

    #[serde(default = "default_locale", skip)]
    pub num_format: CustomFormat,
}

fn default_threads() -> usize {
    10
}

fn default_locale() -> CustomFormat {
    let mut cfb = CustomFormat::builder();
    cfb = cfb.separator(",");
    cfb.build().unwrap_or_default()
}

fn default_blacklist_threshhold() -> usize {
    3
}

fn default_class_icons() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        let mut rng = fastrand::Rng::new();
        let mut base_name = rng.alphabetic().to_ascii_uppercase().to_string();
        for _ in 0..rng.u32(6..8) {
            let c = if rng.bool() {
                rng.alphabetic()
            } else {
                rng.digit(10)
            };
            base_name.push(c)
        }

        Self {
            accounts: vec![],
            theme: AvailableTheme::Dark,
            base_name,
            auto_fetch_newest: true,
            max_threads: 10,
            auto_poll: false,
            show_crawling_restrict: false,
            show_class_icons: true,
            blacklist_threshold: 1,
            num_format: default_locale(),
        }
    }
}

impl Config {
    pub fn get_sso_accounts_mut(
        &mut self,
        name: &str,
    ) -> Option<&mut Vec<SFAccCharacter>> {
        let lower_name = name.to_lowercase();
        self.accounts
            .iter_mut()
            .flat_map(|a| match a {
                AccountConfig::SF {
                    name, characters, ..
                } if name.to_lowercase().trim() == lower_name.trim() => {
                    Some(characters)
                }
                _ => None,
            })
            .next()
    }

    pub fn get_char_conf(
        &self,
        name: &str,
        og_server: ServerID,
    ) -> Option<&CharacterConfig> {
        let mut res = None;

        let lower_name = name.to_lowercase();
        for acc in &self.accounts {
            match acc {
                AccountConfig::Regular {
                    name,
                    server,
                    config,
                    ..
                } => {
                    if ServerIdent::new(server).id != og_server {
                        continue;
                    }
                    if name.to_lowercase().trim() != lower_name.trim() {
                        continue;
                    }
                    res = Some(config);
                    break;
                }
                AccountConfig::SF { characters, .. } => {
                    for c in characters {
                        if ServerIdent::new(&c.ident.server).id != og_server {
                            continue;
                        }
                        if c.ident.name.to_lowercase().trim()
                            != lower_name.trim()
                        {
                            continue;
                        }
                        res = Some(&c.config);
                    }
                }
            }
        }
        res
    }

    pub fn get_char_conf_mut(
        &mut self,
        name: &str,
        og_server: ServerID,
    ) -> Option<&mut CharacterConfig> {
        let mut res = None;

        let lower_name = name.to_lowercase();
        for acc in &mut self.accounts {
            match acc {
                AccountConfig::Regular {
                    name,
                    server,
                    config,
                    ..
                } => {
                    if ServerIdent::new(server).id != og_server {
                        continue;
                    }
                    if name.to_lowercase().trim() != lower_name.trim() {
                        continue;
                    }
                    res = Some(config);
                    break;
                }
                AccountConfig::SF { characters, .. } => {
                    for c in characters {
                        if ServerIdent::new(&c.ident.server).id != og_server {
                            continue;
                        }
                        if c.ident.name.to_lowercase().trim()
                            != lower_name.trim()
                        {
                            continue;
                        }
                        res = Some(&mut c.config);
                    }
                }
            }
        }
        res
    }

    pub fn write(&self) -> Result<(), Box<dyn std::error::Error>> {
        let str = toml::to_string_pretty(self)?;
        std::fs::write("helper.toml", str)?;
        Ok(())
    }
    pub fn restore() -> Result<Self, Box<dyn std::error::Error>> {
        let val = std::fs::read_to_string("helper.toml")?;
        Ok(toml::from_str(&val)?)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum AccountCreds {
    Regular {
        name: String,
        pw_hash: PWHash,
        server: String,
    },
    SF {
        name: String,
        pw_hash: PWHash,
    },
}

impl From<AccountConfig> for AccountCreds {
    fn from(value: AccountConfig) -> Self {
        match value {
            AccountConfig::Regular {
                name,
                pw_hash,
                server,
                ..
            } => AccountCreds::Regular {
                name,
                pw_hash,
                server,
            },
            AccountConfig::SF { name, pw_hash, .. } => {
                AccountCreds::SF { name, pw_hash }
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum AccountConfig {
    Regular {
        name: String,
        pw_hash: PWHash,
        server: String,
        #[serde(flatten)]
        config: CharacterConfig,
    },
    SF {
        name: String,
        pw_hash: PWHash,
        #[serde(default)]
        characters: Vec<SFAccCharacter>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SFAccCharacter {
    pub ident: SFCharIdent,
    pub config: CharacterConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CharacterConfig {
    #[serde(default)]
    pub login: bool,
    #[serde(default)]
    pub auto_battle: bool,
    #[serde(default)]
    pub auto_lure: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Hash, PartialEq, Eq)]
pub struct SFCharIdent {
    pub name: String,
    pub server: String,
}

impl AccountConfig {
    pub fn new(creds: AccountCreds) -> AccountConfig {
        match creds {
            AccountCreds::Regular {
                name,
                pw_hash,
                server,
            } => AccountConfig::Regular {
                name,
                pw_hash,
                server,
                config: Default::default(),
            },
            AccountCreds::SF { name, pw_hash } => AccountConfig::SF {
                name,
                pw_hash,
                characters: Default::default(),
            },
        }
    }
}

#[derive(
    Debug, Serialize, Deserialize, Default, Clone, Copy, PartialEq, Eq,
)]
pub enum AvailableTheme {
    Light,
    #[default]
    Dark,
    Dracula,
    Nord,
    SolarizedLight,
    SolarizedDark,
    GruvboxLight,
    GruvboxDark,
    CatppuccinLatte,
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
    TokyoNight,
    TokyoNightStorm,
    TokyoNightLight,
    KanagawaWave,
    KanagawaDragon,
    KanagawaLotus,
    Moonfly,
    Nightfly,
    Oxocarbon,
}

#[allow(clippy::to_string_trait_impl)]
impl ToString for AvailableTheme {
    fn to_string(&self) -> String {
        use AvailableTheme::*;
        match self {
            Light => Theme::Light,
            Dark => Theme::Dark,
            Dracula => Theme::Dracula,
            Nord => Theme::Nord,
            SolarizedLight => Theme::SolarizedLight,
            SolarizedDark => Theme::SolarizedDark,
            GruvboxLight => Theme::GruvboxLight,
            GruvboxDark => Theme::GruvboxDark,
            CatppuccinLatte => Theme::CatppuccinLatte,
            CatppuccinFrappe => Theme::CatppuccinFrappe,
            CatppuccinMacchiato => Theme::CatppuccinMacchiato,
            CatppuccinMocha => Theme::CatppuccinMocha,
            TokyoNight => Theme::TokyoNight,
            TokyoNightStorm => Theme::TokyoNightStorm,
            TokyoNightLight => Theme::TokyoNightLight,
            KanagawaWave => Theme::KanagawaWave,
            KanagawaDragon => Theme::KanagawaDragon,
            KanagawaLotus => Theme::KanagawaLotus,
            Moonfly => Theme::Moonfly,
            Nightfly => Theme::Nightfly,
            Oxocarbon => Theme::Oxocarbon,
        }
        .to_string()
    }
}

impl AvailableTheme {
    pub fn theme(&self) -> Theme {
        use AvailableTheme::*;

        match self {
            Light => Theme::Light,
            Dark => Theme::Dark,
            Dracula => Theme::Dracula,
            Nord => Theme::Nord,
            SolarizedLight => Theme::SolarizedLight,
            SolarizedDark => Theme::SolarizedDark,
            GruvboxLight => Theme::GruvboxLight,
            GruvboxDark => Theme::GruvboxDark,
            CatppuccinLatte => Theme::CatppuccinLatte,
            CatppuccinFrappe => Theme::CatppuccinFrappe,
            CatppuccinMacchiato => Theme::CatppuccinMacchiato,
            CatppuccinMocha => Theme::CatppuccinMocha,
            TokyoNight => Theme::TokyoNight,
            TokyoNightStorm => Theme::TokyoNightStorm,
            TokyoNightLight => Theme::TokyoNightLight,
            KanagawaWave => Theme::KanagawaWave,
            KanagawaDragon => Theme::KanagawaDragon,
            KanagawaLotus => Theme::KanagawaLotus,
            Moonfly => Theme::Moonfly,
            Nightfly => Theme::Nightfly,
            Oxocarbon => Theme::Oxocarbon,
        }
    }
}
