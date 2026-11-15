use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let path = match args.next() {
        Some(flag) if flag == "--install" && args.next().is_none() => {
            return install();
        }
        Some(flag) if flag == "--config" => {
            let Some(path) = args.next().filter(|_| args.next().is_none()) else {
                eprintln!("Usage: ptyra [--config PATH | --install]");
                return ExitCode::FAILURE;
            };
            Some(PathBuf::from(path))
        }
        Some(_) => {
            eprintln!("Usage: ptyra [--config PATH | --install]");
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

fn install() -> ExitCode {
    let result = (|| {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is not set".to_owned())?;
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        let plan = ptyra::install::InstallPlan::user_local_xdg(&home, &data)
            .map_err(|error| error.to_string())?;
        let receipt = if plan.files()[0]
            .try_exists()
            .map_err(|error| error.to_string())?
        {
            let receipt = plan
                .install_launcher()
                .map_err(|error| format!("{error:?}"))?;
            println!("Using existing {}", plan.files()[0].display());
            receipt
        } else {
            let binary = std::env::current_exe().map_err(|error| error.to_string())?;
            plan.install(&binary)
                .map_err(|error| format!("{error:?}"))?
        };
        for file in receipt.files() {
            println!("Installed {}", file.path().display());
        }
        Ok::<_, String>(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Ptyra install: {error}");
            ExitCode::FAILURE
        }
    }
}
