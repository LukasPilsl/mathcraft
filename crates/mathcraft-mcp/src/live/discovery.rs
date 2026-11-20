//! The discovery file. The app writes `live.json` (port, token, pid) into its config folder with
//! owner-only permissions (0600 on Unix, the user profile's own access rules on Windows); the
//! `mathcraft-mcp` binary reads it. The file is the only place the token is stored.
//!
//! Transport choice: loopback TCP (`127.0.0.1`, random port) with a random bearer token. It is the
//! one mechanism that needs no extra crate and works the same on Linux, macOS and Windows (a Unix
//! socket needs a separate named-pipe code path on Windows). The listener never binds a
//! non-loopback address, and a connection must present the token before any other frame.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// File name inside the discovery folder.
pub const FILE_NAME: &str = "live.json";

/// Environment variable that overrides the discovery folder (tests, unusual setups).
pub const DIR_ENV: &str = "MATHCRAFT_LIVE_DIR";

/// What the app published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

/// The folder of the discovery file: `$MATHCRAFT_LIVE_DIR`, else the app's config folder
/// (`~/Library/Application Support/mathcraft`, `%APPDATA%\mathcraft`, `$XDG_CONFIG_HOME/mathcraft`
/// or `~/.config/mathcraft`).
pub fn default_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    if let Some(dir) = var(DIR_ENV) {
        return Some(PathBuf::from(dir));
    }
    let base = match std::env::consts::OS {
        "macos" => PathBuf::from(var("HOME")?)
            .join("Library")
            .join("Application Support"),
        "windows" => PathBuf::from(var("APPDATA")?),
        _ => match var("XDG_CONFIG_HOME") {
            Some(xdg) => PathBuf::from(xdg),
            None => PathBuf::from(var("HOME")?).join(".config"),
        },
    };
    Some(base.join("mathcraft"))
}

/// A random token of 32 hex characters (128 bits) from the operating system's CSPRNG.
///
/// # Panics
/// When the OS offers no random source at all; a token must never be guessable, so there is no
/// weaker fallback.
pub fn new_token() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).expect("the operating system random source is unavailable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares two tokens without stopping at the first difference.
pub fn tokens_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

/// Writes the discovery file into `dir` (created when missing), readable by the owner only. The
/// content goes to a new file that is renamed over the old one, so a reader never sees half of it.
///
/// # Errors
/// An I/O error.
pub fn write(dir: &Path, info: &Discovery) -> std::io::Result<PathBuf> {
    create_private_dir(dir)?;
    let target = dir.join(FILE_NAME);
    let temp = dir.join(format!(".{FILE_NAME}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    let body = json!({
        "protocol": super::PROTOCOL,
        "port": info.port,
        "token": info.token,
        "pid": info.pid,
    });
    file.write_all(body.to_string().as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, &target)?;
    Ok(target)
}

/// Creates `dir` and its parents; the folder itself gets mode 0700 on Unix.
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// Removes the discovery file when it still names `pid` (a newer app may have replaced it).
pub fn remove(dir: &Path, pid: u32, port: u16) {
    if read(dir).is_some_and(|d| d.pid == pid && d.port == port) {
        let _ = std::fs::remove_file(dir.join(FILE_NAME));
    }
}

/// Reads the discovery file. `None` when it is missing, too big, not JSON or incomplete. On Unix
/// a file that others can read or write is refused: the token in it would not be secret.
pub fn read(dir: &Path) -> Option<Discovery> {
    let path = dir.join(FILE_NAME);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > 4096 {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return None;
        }
    }
    let value: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    if value.get("protocol")?.as_u64()? != u64::from(super::PROTOCOL) {
        return None;
    }
    let token = value.get("token")?.as_str()?.to_string();
    if token.len() < 16 || token.len() > 256 {
        return None;
    }
    Some(Discovery {
        port: u16::try_from(value.get("port")?.as_u64()?).ok()?,
        token,
        pid: u32::try_from(value.get("pid")?.as_u64()?).ok()?,
    })
}

/// [`read`], then checks that the port still accepts a loopback connection. A file left by a
/// crashed app names a dead port and counts as "no app".
pub fn read_alive(dir: &Path) -> Option<Discovery> {
    let found = read(dir)?;
    let addr = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, found.port));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).ok()?;
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mc-disc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn write_read_remove_and_permissions() {
        let dir = temp_dir("rw");
        let info = Discovery {
            port: 4242,
            token: new_token(),
            pid: 7,
        };
        let path = write(&dir, &info).unwrap();
        assert_eq!(read(&dir), Some(info.clone()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
            // A world-readable file is not trusted.
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(read(&dir), None);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        remove(&dir, 8, 4242);
        assert!(read(&dir).is_some());
        remove(&dir, 7, 4242);
        assert!(read(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_file_is_no_app_and_dir_is_private() {
        let dir = temp_dir("stale").join("nested");
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        write(
            &dir,
            &Discovery {
                port: dead,
                token: new_token(),
                pid: 1,
            },
        )
        .unwrap();
        assert!(read(&dir).is_some());
        assert!(read_alive(&dir).is_none());
        let live = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        write(
            &dir,
            &Discovery {
                port: live.local_addr().unwrap().port(),
                token: new_token(),
                pid: 1,
            },
        )
        .unwrap();
        assert!(read_alive(&dir).is_some());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn tokens_are_random_and_compared_whole() {
        assert_ne!(new_token(), new_token());
        assert_eq!(new_token().len(), 32);
        assert!(tokens_equal("abc", "abc"));
        assert!(!tokens_equal("abc", "abd"));
        assert!(!tokens_equal("abc", "abcd"));
    }

    #[test]
    fn garbage_is_not_discovery() {
        let dir = temp_dir("bad");
        std::fs::create_dir_all(&dir).unwrap();
        let bad = [
            "",
            "{",
            "[]",
            "{\"protocol\":1}",
            "{\"protocol\":9,\"port\":1,\"token\":\"aaaaaaaaaaaaaaaaaaaa\",\"pid\":1}",
        ];
        for body in bad {
            let file = dir.join(FILE_NAME);
            std::fs::write(&file, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            assert_eq!(read(&dir), None, "{body}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
