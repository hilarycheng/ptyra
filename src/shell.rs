use crate::error::{AppError, Component};
use std::{
    ffi::{OsStr, OsString},
    io,
    os::unix::ffi::OsStrExt,
    process::Command,
};

pub const MAX_ARGV_BYTES: usize = 64 * 1024;
pub const MAX_ARGS: usize = 256;

/// Builds argv without shell evaluation. No child process is started here.
pub fn shell_command(program: &OsStr, args: &[OsString]) -> Result<Command, AppError> {
    let invalid = || AppError::new(Component::Pty, io::ErrorKind::InvalidInput);
    if program.is_empty() || args.len() > MAX_ARGS {
        return Err(invalid());
    }
    let mut total = 0usize;
    for value in std::iter::once(program).chain(args.iter().map(OsString::as_os_str)) {
        let bytes = value.as_bytes();
        if bytes.contains(&0) {
            return Err(invalid());
        }
        total = total
            .checked_add(bytes.len())
            .and_then(|n| n.checked_add(1))
            .filter(|n| *n <= MAX_ARGV_BYTES)
            .ok_or_else(invalid)?;
    }
    let mut command = Command::new(program);
    command.args(args);
    Ok(command)
}

/// Until terminfo installation and native GUI integration are complete, advertise only dumb output.
/// These overrides affect the child Command, never the application's environment.
pub fn apply_terminal_environment(command: &mut Command) {
    command.env("TERM", "dumb").env_remove("COLORTERM");
}

#[cfg(test)]
mod tests {
    use super::{MAX_ARGS, MAX_ARGV_BYTES, apply_terminal_environment, shell_command};
    use std::{
        ffi::{OsStr, OsString},
        os::unix::ffi::{OsStrExt, OsStringExt},
    };

    #[test]
    fn term_env_matches_supported_features() {
        let mut command = shell_command(OsStr::new("/bin/sh"), &[]).unwrap();
        command.env("TERM", "xterm-kitty");
        command.env("COLORTERM", "truecolor");
        command.env("LANG", "zh_HK.UTF-8");
        apply_terminal_environment(&mut command);
        let get = |name| {
            command
                .get_envs()
                .find(|(key, _)| *key == OsStr::new(name))
                .unwrap()
                .1
        };
        assert_eq!(get("TERM"), Some(OsStr::new("dumb")));
        assert_eq!(get("COLORTERM"), None);
        assert_eq!(get("LANG"), Some(OsStr::new("zh_HK.UTF-8")));
        assert_eq!(command.get_envs().count(), 3);
    }

    #[test]
    fn shell_argv_is_not_reinterpreted() {
        let args = vec![
            OsString::from("--login"),
            OsString::from("spaces ; $(echo secret) `echo secret`\n繁體中文"),
            OsString::from(""),
            OsString::from_vec(vec![0xff, b'x']),
        ];
        let command = shell_command(OsStr::new("/bin/example shell"), &args).unwrap();
        assert_eq!(command.get_program(), OsStr::new("/bin/example shell"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            args.iter().map(OsString::as_os_str).collect::<Vec<_>>()
        );
        let program = OsStr::from_bytes(b"/bin/\xffshell");
        assert_eq!(
            shell_command(program, &[])
                .unwrap()
                .get_program()
                .as_bytes(),
            program.as_bytes()
        );
        assert!(shell_command(OsStr::new(""), &[]).is_err());
        assert!(shell_command(OsStr::from_bytes(b"/bin/sh\0bad"), &[]).is_err());
        assert!(
            shell_command(
                OsStr::new("/bin/sh"),
                &[OsString::from_vec(b"bad\0arg".to_vec())]
            )
            .is_err()
        );
        assert!(
            shell_command(
                OsStr::new("/bin/sh"),
                &[OsString::from("x".repeat(MAX_ARGV_BYTES))]
            )
            .is_err()
        );
        assert!(
            shell_command(OsStr::new("/bin/sh"), &vec![OsString::new(); MAX_ARGS + 1]).is_err()
        );
    }
}
