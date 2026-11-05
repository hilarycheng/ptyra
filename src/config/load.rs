use super::{Config, ConfigError, MAX_CONFIG_BYTES, error};
use std::{
    fmt,
    fs::File,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct ConfigDiagnostic {
    pub path: PathBuf,
    pub error: ConfigError,
}

impl fmt::Display for ConfigDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}: {}: {}",
            self.path, self.error.field, self.error.reason
        )?;
        if let Some(offset) = self.error.offset {
            write!(f, " (byte {offset})")?;
        }
        Ok(())
    }
}

fn diagnostic(path: &Path, error: ConfigError) -> ConfigDiagnostic {
    ConfigDiagnostic {
        path: path.to_owned(),
        error,
    }
}

pub fn read_config(path: &Path, reader: impl Read) -> Result<Config, ConfigDiagnostic> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| diagnostic(path, error("config", "cannot read config")))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(diagnostic(path, error("config", "file exceeds 64 KiB")));
    }
    let source = std::str::from_utf8(&bytes)
        .map_err(|_| diagnostic(path, error("config", "expected UTF-8")))?;
    Config::from_toml(Some(source)).map_err(|error| diagnostic(path, error))
}

/// Read-only; a missing optional config uses defaults. All other failures return visible diagnostics.
pub fn load_config(path: &Path) -> Result<Config, ConfigDiagnostic> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Config::default()),
        Err(_) => return Err(diagnostic(path, error("config", "cannot open config"))),
    };
    if !file
        .metadata()
        .map_err(|_| diagnostic(path, error("config", "cannot inspect config")))?
        .is_file()
    {
        return Err(diagnostic(path, error("config", "expected a regular file")));
    }
    read_config(path, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_config_returns_diagnostic() {
        let path = Path::new("/example/config.toml");
        for input in [
            b"font_size = 'SECRET'".as_slice(),
            b"font_size = [SECRET",
            b"font_family = \xff",
        ] {
            let diagnostic = read_config(path, input).unwrap_err();
            assert_eq!(diagnostic.path, path);
            let text = format!("{diagnostic} {diagnostic:?}");
            assert!(text.contains("config.toml"));
            assert!(!text.contains("SECRET"));
        }
        let too_large = vec![b' '; MAX_CONFIG_BYTES + 1];
        assert!(read_config(path, too_large.as_slice()).is_err());
        assert_eq!(
            read_config(path, b"".as_slice()).unwrap(),
            Config::default()
        );
        assert_eq!(
            load_config(Path::new("/nonexistent-ptyra-test/config.toml")).unwrap(),
            Config::default()
        );
    }
}
