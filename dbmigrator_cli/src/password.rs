//! Database connection with libpq-style password lookup: the URL, then `PGPASSWORD`, then the
//! password file, then a prompt on the terminal when the server asks for a password.

use crate::cli::CliError;
use console::Term;
use dbmigrator::tokio_postgres::config::Host;
use dbmigrator::tokio_postgres::error::SqlState;
use dbmigrator::tokio_postgres::{Config as PgConfig, Error as PgError};
use dbmigrator::{AsyncDriver, MigratorError};
use std::path::{Path, PathBuf};

const DEFAULT_PORT: u16 = 5432;

/// Socket directories libpq is commonly built with; the password file matches them as `localhost`.
#[cfg(unix)]
const DEFAULT_SOCKET_DIRS: &[&str] = &["/tmp", "/var/run/postgresql", "/run/postgresql"];

/// Connects to `db_url`, asking for the password on the terminal unless `no_prompt` is set.
pub async fn connect(db_url: &str, no_prompt: bool) -> Result<AsyncDriver, CliError> {
    let mut config: PgConfig = db_url.parse().map_err(MigratorError::from)?;
    let user = config
        .get_user()
        .map(str::to_owned)
        .or_else(|| whoami::username().ok());

    if is_empty(config.get_password()) {
        let password = env_password().or_else(|| {
            user.as_deref()
                .and_then(|user| password_file(&config, user))
        });
        if let Some(password) = password {
            config.password(password);
        }
    }

    match AsyncDriver::connect_config(&config).await {
        Err(MigratorError::PgError(e))
            if !no_prompt
                && needs_password(&e, is_empty(config.get_password()))
                && Term::stderr().is_term() =>
        {
            match prompt(user.as_deref()) {
                Some(password) => {
                    config.password(password);
                    Ok(AsyncDriver::connect_config(&config).await?)
                }
                None => Err(MigratorError::PgError(e).into()),
            }
        }
        result => Ok(result?),
    }
}

fn is_empty(password: Option<&[u8]>) -> bool {
    password.is_none_or(<[u8]>::is_empty)
}

/// Whether the connection failed only for lack of a password.
fn needs_password(e: &PgError, password_empty: bool) -> bool {
    // tokio-postgres keeps the error kind private, and since 0.7.15 its Display prints only
    // "invalid configuration"; the reason is in the source chain.
    let mut source = std::error::Error::source(e);
    while let Some(cause) = source {
        if cause.to_string().contains("password missing") {
            return true;
        }
        source = cause.source();
    }
    // `postgres://user:@host` sets an empty password, which `Config` cannot unset.
    password_empty && e.code() == Some(&SqlState::INVALID_PASSWORD)
}

/// Reads a password without echo; `None` on an empty answer or when the terminal cannot be read.
fn prompt(user: Option<&str>) -> Option<String> {
    let term = Term::stderr();
    let label = match user {
        Some(user) => format!("Password for user {user}: "),
        None => "Password: ".to_string(),
    };
    term.write_str(&label).ok()?;
    term.read_secure_line().ok().filter(|p| !p.is_empty())
}

fn env_password() -> Option<Vec<u8>> {
    std::env::var_os("PGPASSWORD")
        .filter(|p| !p.is_empty())
        .map(|p| p.into_encoded_bytes())
}

/// Password for the first host of `config` from the libpq password file.
fn password_file(config: &PgConfig, user: &str) -> Option<Vec<u8>> {
    let host = lookup_host(config.get_hosts().first()?);
    let port = config
        .get_ports()
        .first()
        .copied()
        .unwrap_or(DEFAULT_PORT)
        .to_string();
    let dbname = config.get_dbname().unwrap_or(user);
    let content = read_password_file(&password_file_path()?)?;
    lookup(
        &content,
        &host,
        port.as_bytes(),
        dbname.as_bytes(),
        user.as_bytes(),
    )
}

fn lookup_host(host: &Host) -> Vec<u8> {
    match host {
        Host::Tcp(host) => host.as_bytes().to_vec(),
        #[cfg(unix)]
        Host::Unix(path) => {
            if DEFAULT_SOCKET_DIRS.iter().any(|dir| path == Path::new(dir)) {
                b"localhost".to_vec()
            } else {
                path.as_os_str().as_encoded_bytes().to_vec()
            }
        }
    }
}

fn password_file_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PGPASSFILE").filter(|p| !p.is_empty()) {
        return Some(path.into());
    }
    #[cfg(windows)]
    let path = std::env::var_os("APPDATA")
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(&d).join("postgresql").join("pgpass.conf"));
    #[cfg(not(windows))]
    let path = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| Path::new(&h).join(".pgpass"));
    path
}

/// Reads the password file, skipping it with libpq's warnings when it is not safe to use.
fn read_password_file(path: &Path) -> Option<Vec<u8>> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        eprintln!(
            "WARNING: password file \"{}\" is not a plain file",
            path.display()
        );
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            eprintln!(
                "WARNING: password file \"{}\" has group or world access; permissions should be u=rw (0600) or less",
                path.display()
            );
            return None;
        }
    }
    std::fs::read(path).ok()
}

/// Password of the first line matching the connection, as libpq's `passwordFromFile` does.
fn lookup(content: &[u8], host: &[u8], port: &[u8], dbname: &[u8], user: &[u8]) -> Option<Vec<u8>> {
    'lines: for line in content.split(|&b| b == b'\n') {
        let mut rest = line;
        while let Some(stripped) = rest.strip_suffix(b"\r") {
            rest = stripped;
        }
        if rest.is_empty() || rest[0] == b'#' {
            continue;
        }
        for token in [host, port, dbname, user] {
            match match_field(rest, token) {
                Some(next) => rest = next,
                None => continue 'lines,
            }
        }
        return Some(unescape_password(rest));
    }
    None
}

/// Matches one `:`-terminated field against `token`, returning the rest of the line after it.
fn match_field<'a>(line: &'a [u8], token: &[u8]) -> Option<&'a [u8]> {
    if let Some(rest) = line.strip_prefix(b"*:") {
        return Some(rest);
    }
    let (mut i, mut t) = (0, 0);
    while i < line.len() {
        let escaped = line[i] == b'\\';
        if escaped {
            i += 1;
        }
        let c = *line.get(i)?;
        if c == b':' && !escaped && t == token.len() {
            return Some(&line[i + 1..]);
        }
        if token.get(t) != Some(&c) {
            return None;
        }
        i += 1;
        t += 1;
    }
    None
}

fn unescape_password(field: &[u8]) -> Vec<u8> {
    let mut password = Vec::new();
    let mut i = 0;
    while i < field.len() && field[i] != b':' {
        if field[i] == b'\\' && i + 1 < field.len() {
            i += 1;
        }
        password.push(field[i]);
        i += 1;
    }
    password
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn find(content: &[u8], host: &str, port: &str, dbname: &str, user: &str) -> Option<Vec<u8>> {
        lookup(
            content,
            host.as_bytes(),
            port.as_bytes(),
            dbname.as_bytes(),
            user.as_bytes(),
        )
    }

    #[test]
    fn exact_and_wildcard_fields() {
        let file = b"db1:5432:app:alice:secret1\n*:*:*:bob:secret2\n";
        assert_eq!(
            find(file, "db1", "5432", "app", "alice"),
            Some(b"secret1".to_vec())
        );
        assert_eq!(
            find(file, "db2", "6432", "x", "bob"),
            Some(b"secret2".to_vec())
        );
        assert_eq!(find(file, "db2", "5432", "app", "alice"), None);
    }

    #[test]
    fn wildcard_only_as_whole_field() {
        assert_eq!(find(b"db*:*:*:*:pw\n", "db1", "5432", "app", "u"), None);
        assert_eq!(
            find(b"db*:*:*:*:pw\n", "db*", "5432", "app", "u"),
            Some(b"pw".to_vec())
        );
    }

    #[test]
    fn escapes() {
        let file = br"\:\:1:*:*:*:a\:b\\c";
        assert_eq!(
            find(file, "::1", "5432", "app", "u"),
            Some(br"a:b\c".to_vec())
        );
        assert_eq!(
            find(br"h:*:*:*:pw\", "h", "1", "d", "u"),
            Some(br"pw\".to_vec())
        );
    }

    #[test]
    fn comments_blank_lines_and_crlf() {
        let file = b"# h:*:*:*:commented\n\n h:*:*:*:indented\r\nh:*:*:*:pw\r\n";
        assert_eq!(find(file, "h", "5432", "d", "u"), Some(b"pw".to_vec()));
        assert_eq!(
            find(file, " h", "5432", "d", "u"),
            Some(b"indented".to_vec())
        );
        assert_eq!(find(file, "# h", "5432", "d", "u"), None);
    }

    #[test]
    fn first_match_wins_even_if_empty() {
        let file = b"h:*:*:u:\nh:*:*:u:second\n";
        assert_eq!(find(file, "h", "5432", "d", "u"), Some(Vec::new()));
    }

    #[test]
    fn extra_columns_and_short_lines() {
        let file = b"h:*:*:u\nh:*:*:u:pw:extra\n";
        assert_eq!(find(file, "h", "5432", "d", "u"), Some(b"pw".to_vec()));
    }

    #[test]
    fn non_utf8_bytes() {
        let file = b"h:*:*:u:\xb1\xea\n";
        assert_eq!(
            find(file, "h", "5432", "d", "u"),
            Some(b"\xb1\xea".to_vec())
        );
    }

    #[cfg(unix)]
    #[test]
    fn socket_host_key() {
        let default = Host::Unix(PathBuf::from("/var/run/postgresql"));
        let custom = Host::Unix(PathBuf::from("/srv/pg"));
        assert_eq!(lookup_host(&default), b"localhost".to_vec());
        assert_eq!(lookup_host(&custom), b"/srv/pg".to_vec());
        assert_eq!(lookup_host(&Host::Tcp("db".into())), b"db".to_vec());
    }

    #[cfg(unix)]
    #[test]
    fn password_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pgpass");
        std::fs::write(&path, b"*:*:*:*:pw\n").unwrap();

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_password_file(&path), Some(b"*:*:*:*:pw\n".to_vec()));

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read_password_file(&path), None);

        assert_eq!(read_password_file(dir.path()), None);
    }

    /// Guards the "password missing" detection against tokio-postgres changing its error text.
    #[tokio::test]
    async fn detects_server_password_request() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut len = [0u8; 4];
            stream.read_exact(&mut len).unwrap();
            let mut startup = vec![0u8; u32::from_be_bytes(len) as usize - 4];
            stream.read_exact(&mut startup).unwrap();
            // AuthenticationCleartextPassword
            stream.write_all(&[b'R', 0, 0, 0, 8, 0, 0, 0, 3]).unwrap();
        });

        let mut config = PgConfig::new();
        config
            .host("127.0.0.1")
            .port(port)
            .user("u")
            .ssl_mode(dbmigrator::tokio_postgres::config::SslMode::Disable);
        let result = AsyncDriver::connect_config(&config).await;
        server.join().unwrap();
        match result {
            Err(MigratorError::PgError(e)) => assert!(needs_password(&e, false), "{e:?}"),
            Err(e) => panic!("unexpected error: {e}"),
            Ok(_) => panic!("connected without a password"),
        }
    }
}
