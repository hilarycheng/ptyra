use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let path = match args.next() {
        Some(flag) if flag == "--config" => {
            let Some(path) = args.next().filter(|_| args.next().is_none()) else {
                eprintln!("Usage: ptyra [--config PATH]");
                return ExitCode::FAILURE;
            };
            Some(PathBuf::from(path))
        }
        Some(_) => {
            eprintln!("Usage: ptyra [--config PATH]");
            return ExitCode::FAILURE;
        }
        None => std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .map(|base| base.join("ptyra/config.toml")),
    };
    let config = match path
        .map(|path| ptyra::config::load::load_config(&path))
        .transpose()
    {
        Ok(config) => config.unwrap_or_default(),
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    match ptyra::runtime::run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Ptyra: {error}");
            ExitCode::FAILURE
        }
    }
}
