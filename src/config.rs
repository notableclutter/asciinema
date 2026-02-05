use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use config::{self, File};
use reqwest::Url;
use serde::Deserialize;
use uuid::Uuid;

use crate::status;

const DEFAULT_SERVER_URL: &str = "https://asciinema.org";
const INSTALL_ID_FILENAME: &str = "install-id";

pub type Key = Option<Vec<u8>>;

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Config {
    server: Server,
    pub session: Session,
    pub playback: Playback,
    pub notifications: Notifications,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Server {
    url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(unused)]
pub struct Session {
    pub command: Option<String>,
    pub capture_input: bool,
    pub capture_env: Option<String>,
    pub idle_time_limit: Option<f64>,
    pub prefix_key: Option<String>,
    pub pause_key: Option<String>,
    pub add_marker_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(unused)]
pub struct Playback {
    pub speed: Option<f64>,
    pub idle_time_limit: Option<f64>,
    pub pause_key: Option<String>,
    pub step_key: Option<String>,
    pub next_marker_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Notifications {
    pub enabled: bool,
    pub command: Option<String>,
}

impl Config {
    pub fn new(server_url: Option<String>) -> Result<Self> {
        let mut config = config::Config::builder()
            .set_default("server.url", None::<Option<String>>)?
            .set_default("playback.speed", None::<Option<f64>>)?
            .set_default("session.capture_input", false)?
            .set_default("notifications.enabled", true)?
            .add_source(File::with_name("/etc/asciinema/config.toml").required(false))
            .add_source(File::with_name(&user_defaults_path()?.to_string_lossy()).required(false))
            .add_source(File::with_name(&user_config_path()?.to_string_lossy()).required(false));

        // legacy env var
        if let Ok(url) = env::var("ASCIINEMA_API_URL") {
            config = config.set_override("server.url", Some(url))?;
        }

        if let Ok(url) = env::var("ASCIINEMA_SERVER_URL") {
            config = config.set_override("server.url", Some(url))?;
        }

        if let Some(url) = server_url {
            config = config.set_override("server.url", Some(url))?;
        }

        Ok(config.build()?.try_deserialize()?)
    }

    pub fn get_server_url(&mut self) -> Result<Url> {
        match self.server.url.as_ref() {
            Some(url) => Ok(parse_server_url(url)?),

            None => {
                let url = parse_server_url(&ask_for_server_url()?)?;
                save_default_server_url(url.as_ref())?;
                self.server.url = Some(url.to_string());

                Ok(url)
            }
        }
    }

    pub fn get_install_id(&self) -> Result<String> {
        let path = install_id_path()?;
        let legacy_path = legacy_install_id_path()?;

        if let Some(id) = read_install_id(&path)? {
            Ok(id)
        } else if let Some(id) = read_install_id(&legacy_path)? {
            Ok(id)
        } else {
            let id = generate_install_id();
            save_install_id(&path, &id)?;

            Ok(id)
        }
    }
}

impl Session {
    pub fn prefix_key(&self) -> Result<Option<Key>> {
        self.prefix_key.as_ref().map(parse_key).transpose()
    }

    pub fn pause_key(&self) -> Result<Option<Key>> {
        self.pause_key.as_ref().map(parse_key).transpose()
    }

    pub fn add_marker_key(&self) -> Result<Option<Key>> {
        self.add_marker_key.as_ref().map(parse_key).transpose()
    }
}

impl Playback {
    pub fn pause_key(&self) -> Result<Option<Key>> {
        self.pause_key.as_ref().map(parse_key).transpose()
    }

    pub fn step_key(&self) -> Result<Option<Key>> {
        self.step_key.as_ref().map(parse_key).transpose()
    }

    pub fn next_marker_key(&self) -> Result<Option<Key>> {
        self.next_marker_key.as_ref().map(parse_key).transpose()
    }
}

fn ask_for_server_url() -> Result<String> {
    println!("No asciinema server configured for this CLI.");

    let url = rustyline::DefaultEditor::new()?.readline_with_initial(
        "Enter the server URL to use by default: ",
        (DEFAULT_SERVER_URL, ""),
    )?;

    println!();

    Ok(url)
}

fn save_default_server_url(url: &str) -> Result<()> {
    let path = user_defaults_path()?;

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }

    fs::write(path, format!("[server]\nurl = \"{url}\"\n"))?;

    Ok(())
}

fn parse_server_url(s: &str) -> Result<Url> {
    let url = Url::parse(s)?;

    if url.host().is_none() {
        bail!("server URL is missing a host");
    }

    Ok(url)
}

fn read_install_id(path: &PathBuf) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s.trim().to_string())),

        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                Ok(None)
            } else {
                bail!(e)
            }
        }
    }
}

fn generate_install_id() -> String {
    Uuid::new_v4().to_string()
}

fn save_install_id(path: &PathBuf, id: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }

    fs::write(path, id)?;

    Ok(())
}

pub fn user_config_path() -> Result<PathBuf> {
    Ok(config_home()?.join("config.toml"))
}

fn legacy_user_config_path() -> Result<PathBuf> {
    Ok(config_home()?.join("config"))
}

fn user_defaults_path() -> Result<PathBuf> {
    Ok(config_home()?.join("defaults.toml"))
}

fn install_id_path() -> Result<PathBuf> {
    Ok(state_home()?.join(INSTALL_ID_FILENAME))
}

fn legacy_install_id_path() -> Result<PathBuf> {
    Ok(config_home()?.join(INSTALL_ID_FILENAME))
}

fn config_home() -> Result<PathBuf> {
    env::var("ASCIINEMA_CONFIG_HOME")
        .map(PathBuf::from)
        .or(env::var("XDG_CONFIG_HOME").map(|home| Path::new(&home).join("asciinema")))
        .or(env::var("HOME").map(|home| Path::new(&home).join(".config").join("asciinema")))
        .map_err(|_| anyhow!("need $HOME or $XDG_CONFIG_HOME or $ASCIINEMA_CONFIG_HOME"))
}

fn state_home() -> Result<PathBuf> {
    env::var("ASCIINEMA_STATE_HOME")
        .map(PathBuf::from)
        .or(env::var("XDG_STATE_HOME").map(|home| Path::new(&home).join("asciinema")))
        .or(env::var("HOME").map(|home| {
            Path::new(&home)
                .join(".local")
                .join("state")
                .join("asciinema")
        }))
        .map_err(|_| anyhow!("need $HOME or $XDG_STATE_HOME or $ASCIINEMA_STATE_HOME"))
}

fn parse_key<S: AsRef<str>>(key: S) -> Result<Key> {
    let key = key.as_ref();
    let chars: Vec<char> = key.chars().collect();

    if let Some(fkey) = parse_function_key(key) {
        return Ok(Some(fkey));
    }

    if let Some(byte) = parse_hex_key(key) {
        return Ok(Some(vec![byte]));
    }

    match chars.len() {
        0 => return Ok(None),

        1 => {
            let mut buf = [0; 4];
            let str = chars[0].encode_utf8(&mut buf);

            return Ok(Some(str.as_bytes().into()));
        }

        2 => {
            if chars[0] == '^' && chars[1].is_ascii() {
                if let Some(ctrl_byte) = ctrl_byte(chars[1]) {
                    return Ok(Some(vec![ctrl_byte]));
                }
            }
        }

        3 => {
            if chars[0].eq_ignore_ascii_case(&'C')
                && ['+', '-'].contains(&chars[1])
                && chars[2].is_ascii()
            {
                if let Some(ctrl_byte) = ctrl_byte(chars[2]) {
                    return Ok(Some(vec![ctrl_byte]));
                }
            }
        }

        _ => (),
    }

    Err(anyhow!("invalid key definition '{key}'"))
}

fn ctrl_byte(input: char) -> Option<u8> {
    if !input.is_ascii() {
        return None;
    }

    if input == '?' {
        return Some(0x7f);
    }

    let upper = input.to_ascii_uppercase();
    if upper.is_ascii_alphabetic() {
        return Some(upper as u8 - 0x40);
    }

    match upper {
        '@' | '[' | '\\' | ']' | '^' | '_' => Some((upper as u8) & 0x1f),
        '0'..='9' => Some((upper as u8) & 0x1f),
        _ => None,
    }
}

fn parse_hex_key(key: &str) -> Option<u8> {
    let hex_str = key.strip_prefix("0x")?;

    if hex_str.is_empty() || hex_str.len() > 2 || !hex_str.chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }

    u8::from_str_radix(hex_str, 16).ok()
}

fn parse_function_key(key: &str) -> Option<Vec<u8>> {
    let key = key.trim();

    if !key.starts_with("C-") && !key.starts_with("c-") {
        return None;
    }

    let inner = key.strip_prefix("C-").or_else(|| key.strip_prefix("c-"))?;

    if !inner.starts_with('<') || !inner.ends_with('>') {
        return None;
    }

    let label = &inner[1..inner.len() - 1];
    let mut chars = label.chars();
    let first = chars.next()?;

    if !first.eq_ignore_ascii_case(&'f') {
        return None;
    }

    let number: u8 = chars.collect::<String>().parse().ok()?;

    build_ctrl_function_key(number)
}

fn build_ctrl_function_key(number: u8) -> Option<Vec<u8>> {
    match number {
        1 => Some(b"\x1b[1;5P".to_vec()),
        2 => Some(b"\x1b[1;5Q".to_vec()),
        3 => Some(b"\x1b[1;5R".to_vec()),
        4 => Some(b"\x1b[1;5S".to_vec()),
        5 => Some(b"\x1b[15;5~".to_vec()),
        6 => Some(b"\x1b[17;5~".to_vec()),
        7 => Some(b"\x1b[18;5~".to_vec()),
        8 => Some(b"\x1b[19;5~".to_vec()),
        9 => Some(b"\x1b[20;5~".to_vec()),
        10 => Some(b"\x1b[21;5~".to_vec()),
        11 => Some(b"\x1b[23;5~".to_vec()),
        12 => Some(b"\x1b[24;5~".to_vec()),
        _ => None,
    }
}

pub fn check_legacy_config_file() {
    let Ok(legacy_path) = legacy_user_config_path() else {
        return;
    };

    let Ok(new_path) = user_config_path() else {
        return;
    };

    if legacy_path.exists() && !new_path.exists() {
        status::warning!(
            "Your config file at {} uses the location and format from asciinema 2.x.",
            legacy_path.to_string_lossy()
        );

        status::warning!(
            "For asciinema 3.x (this version) create a new config file at {}.",
            new_path.to_string_lossy()
        );

        status::warning!("Read the documentation (CLI -> Configuration) for details.\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_empty() {
        assert_eq!(parse_key("").unwrap(), None);
    }

    #[test]
    fn test_parse_key_single_char() {
        assert_eq!(parse_key("a").unwrap(), Some(vec![b'a']));
        assert_eq!(parse_key(" ").unwrap(), Some(vec![b' ']));
    }

    #[test]
    fn test_parse_key_ctrl_alpha() {
        assert_eq!(parse_key("^a").unwrap(), Some(vec![0x01]));
        assert_eq!(parse_key("^A").unwrap(), Some(vec![0x01]));
        assert_eq!(parse_key("^w").unwrap(), Some(vec![0x17]));
        assert_eq!(parse_key("^z").unwrap(), Some(vec![0x1a]));
    }

    #[test]
    fn test_parse_key_ctrl_non_alpha() {
        assert_eq!(parse_key("^]").unwrap(), Some(vec![0x1d]));
        assert_eq!(parse_key("^\\").unwrap(), Some(vec![0x1c]));
        assert_eq!(parse_key("^6").unwrap(), Some(vec![0x16]));
    }

    #[test]
    fn test_parse_key_c_format() {
        assert_eq!(parse_key("C-a").unwrap(), Some(vec![0x01]));
        assert_eq!(parse_key("C-A").unwrap(), Some(vec![0x01]));
        assert_eq!(parse_key("C-]").unwrap(), Some(vec![0x1d]));
    }

    #[test]
    fn test_parse_key_function_keys() {
        assert_eq!(
            parse_key("C-<f10>").unwrap(),
            Some(vec![0x1b, 0x5b, 0x32, 0x31, 0x3b, 0x35, 0x7e])
        );
        assert_eq!(
            parse_key("C-<f11>").unwrap(),
            Some(vec![0x1b, 0x5b, 0x32, 0x33, 0x3b, 0x35, 0x7e])
        );
        assert_eq!(
            parse_key("C-<f12>").unwrap(),
            Some(vec![0x1b, 0x5b, 0x32, 0x34, 0x3b, 0x35, 0x7e])
        );
    }

    #[test]
    fn test_parse_key_invalid() {
        assert!(parse_key("invalid").is_err());
    }
}
