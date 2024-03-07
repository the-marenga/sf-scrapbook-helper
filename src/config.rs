use iced::Theme;
use serde::{Deserialize, Serialize};
use sf_api::session::PWHash;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub accounts: Vec<CharacterConfig>,
    pub theme: AvailableTheme,
    pub base_name: String,
    pub auto_fetch_newest: bool,
    #[serde(default = "default_threads")]
    pub max_threads: usize,
}

fn default_threads() -> usize {
    10
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
        }
    }
}

impl Config {
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CharacterConfig {
    #[serde(flatten)]
    pub creds: AccountCreds,
}
impl CharacterConfig {
    pub fn new(creds: AccountCreds) -> CharacterConfig {
        CharacterConfig { creds }
    }
}

#[derive(
    Debug, Serialize, Deserialize, Default, Clone, Copy, PartialEq, Eq,
)]
pub enum AvailableTheme {
    #[default]
    Light,
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
