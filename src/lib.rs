use log::info;
use std::fs::File;
use std::io::{self, Read};
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
