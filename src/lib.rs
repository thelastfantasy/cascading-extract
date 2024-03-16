use log::info;
use serde::Deserialize;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use sevenz_rust::default_entry_extract_fn;

pub fn is_7z<P: AsRef<Path>>(path: P) -> io::Result<bool> {
    let mut buf = [0; 8]; // 7z files have a signature in the first few bytes
    let mut file = File::open(path)?;
    file.read_exact(&mut buf)?;
    Ok(infer::archive::is_7z(&buf))
}

pub fn try_extract_7z_with_password<P: AsRef<Path>>(
    path: P,
    password: &str,
    dest: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    sevenz_rust::decompress_with_extract_fn_and_password(
        File::open(&path).unwrap(),
        dest,
        password.into(),
        |entry, reader, dest| {
            info!("开始解压 {}", entry.name());
            let r = default_entry_extract_fn(entry, reader, dest);
            info!("解压完成 {}", entry.name());
            r
        },
    )
    .map_err(|e| e.into())
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(rename = "config")]
    pub config: ConfigSettings,
    #[serde(rename = "user")]
    pub user: UserConfig,
}

#[derive(Debug, Deserialize)]
pub struct ConfigSettings {
    pub delete_archive: bool,
    pub recursive_search: bool,
    #[serde(default = "default_threads")]
    pub threads: u8,
    pub dest: String,
    pub smart_mode: bool,
}

fn default_threads() -> u8 {
    4
}

#[derive(Debug, Deserialize)]
pub struct UserConfig {
    pub passwords: Option<Vec<String>>,
    pub watch_folders: Option<Vec<String>>,
}

pub fn read_config() -> Result<Config, Box<dyn std::error::Error>> {
    let settings_file = File::open("settings.toml")?;
    let mut buf_reader = BufReader::new(settings_file);

    let mut settings_content = String::new();
    buf_reader.read_to_string(&mut settings_content)?;

    let mut settings: Config = toml::from_str(&settings_content)?;

    if settings.config.threads < 1 || settings.config.threads > 8 {
        settings.config.threads = default_threads();
    }

    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_config() {
        let config = read_config().unwrap();
        assert_eq!(config.config.delete_archive, true);
        assert_eq!(config.config.recursive_search, true);
        assert_eq!(config.config.threads, 4);
        assert_eq!(config.config.dest, "".to_string());
        assert_eq!(config.config.smart_mode, true);
        assert_eq!(config.user.passwords.unwrap()[0], "1151".to_string());
        assert_eq!(config.user.watch_folders.unwrap().len(), 0);
    }
}
