use log::info;
use serde::Deserialize;
use std::env;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::ops::Deref;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, Semaphore};

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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

pub fn read_7z_contents<P: AsRef<Path>>(
    path: P,
) -> Result<Vec<sevenz_rust::SevenZArchiveEntry>, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let password = sevenz_rust::Password::empty();
    let archive = sevenz_rust::Archive::read(&mut file, len, password.as_slice())?;

    Ok(archive.files)
}

pub fn should_create_folder_when_extract_with_smart_mode<P: AsRef<Path>>(
    path: P,
) -> Result<bool, Box<dyn std::error::Error>> {
    let files = read_7z_contents(path)?;

    let mut root_file_count = 0;
    let mut root_directory_count = 0;
    let mut should_create = false;

    for file in &files {
        if root_file_count > 1 || root_directory_count > 1 {
            break;
        }

        if file.is_directory() {
            if file.name().contains('/') {
                continue;
            } else {
                root_directory_count += 1;
            }
        } else {
            if file.name().contains('/') {
                continue;
            } else {
                root_file_count += 1;
            }
        }
    }

    if root_file_count > 1 || root_directory_count > 1 {
        should_create = true;
    }

    Ok(should_create)
}

pub fn delete_archive<P: AsRef<Path>>(path: P) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::remove_file(path)?;
    Ok(())
}

pub async fn start_extraction<P: AsRef<Path> + Send + Sync + 'static>(
    paths: Arc<[P]>,
    passwords: Vec<Arc<String>>,
    dest: P,
    max_threads: usize,
) {
    let semaphore = Arc::new(Mutex::new(Semaphore::new(max_threads)));
    let stop_flag = Arc::new(AtomicBool::new(false)); // Moved stop_flag outside the loop

    // Explicitly specify the type parameter for the Sender
    let (tx, mut rx) = mpsc::channel::<String>(passwords.len());

    for password in passwords {
        let tx = tx.clone();
        let dest = dest.as_ref().to_owned();
        let paths = paths.clone();
        let semaphore = semaphore.clone(); // Clone the Arc

        let stop_flag = stop_flag.clone(); // Clone the Arc

        tokio::spawn(async move {
            let semaphore_ref = semaphore.lock().await;
            let permit = semaphore_ref.acquire().await.unwrap();

            for path in paths.iter() {
                if let Ok(()) = try_extract_7z_with_password(&path, &password, &dest) {
                    info!("解压成功: {}", path.as_ref().to_string_lossy());
                    info!("找到正确的密码: {}", password);
                    stop_flag.store(true, Ordering::Relaxed);
                    tx.send(password.deref().clone()).await.unwrap();
                    drop(permit);
                    return;
                }
            }

            drop(permit);
        });
    }

    // Wait for the first successful extraction or stop flag
    while let Some(password) = rx.recv().await {
        info!("找到正确的密码: {}", password);
        if stop_flag.load(Ordering::Relaxed) {
            info!("发现正确密码，终止其他任务队列");
            break;
        }
    }
}

pub fn extract_to_temp_folder<P: AsRef<Path> + Send + Sync>(
    path: P,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let temp_dir = env::temp_dir().join("cascading-extract");
    let temp_dir_path = temp_dir.to_path_buf();
    std::fs::create_dir_all(&temp_dir_path)?;
    try_extract_7z_with_password(&path, "", &temp_dir_path)?;
    Ok(temp_dir_path)
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
    use sevenz_rust::Password;

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

    #[test]
    fn test_is_7z() {
        assert_eq!(is_7z("tests/sample.7z").unwrap(), true);
        assert_eq!(is_7z("tests/7ziplogo_p.7z").unwrap(), true);
        assert_eq!(is_7z("tests/7ziplogo.7z").unwrap(), true);
        assert_eq!(is_7z("tests/7zFormat.txt").unwrap(), false);
        assert_eq!(is_7z("tests/7ziplogo.png").unwrap(), false);
    }

    #[test]
    fn show_7z_content() {
        let mut file = std::fs::File::open("tests/sample.7z").unwrap();
        let len = file.metadata().unwrap().len();
        let password = Password::empty();
        let archive = sevenz_rust::Archive::read(&mut file, len, password.as_slice()).unwrap();
        let folder_count = archive.folders.len();

        // println!("{:?}", archive.folders);
        // println!("{:?}", archive.files);
        for file in archive.files {
            println!("{:?}", file.name());
            println!("{:?}", file.is_directory());
        }
        assert_eq!(folder_count, 1);
    }

    #[test]
    fn test_should_create_folder_when_extract_with_smart_mode() {
        assert_eq!(
            should_create_folder_when_extract_with_smart_mode("tests/sample.7z").unwrap(),
            true
        );
        assert_eq!(
            should_create_folder_when_extract_with_smart_mode("tests/7ziplogo.7z").unwrap(),
            false
        );
    }

    #[test]
    fn test_my_logger() {
        my_logger::init();
        info!("test");
        log::error!("test");
        log::warn!("test");
    }
}
