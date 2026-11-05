use std::{error::Error, fmt, io};

/// Fixed component names keep untrusted payloads out of error context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Wayland,
    Pty,
    Terminal,
    Renderer,
    Clipboard,
    Ime,
    Config,
}

impl fmt::Display for Component {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Wayland => "wayland",
            Self::Pty => "pty",
            Self::Terminal => "terminal",
            Self::Renderer => "renderer",
            Self::Clipboard => "clipboard",
            Self::Ime => "ime",
            Self::Config => "config",
        })
    }
}

/// Stores diagnostic metadata only, never the original error message or source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppError {
    component: Component,
    kind: io::ErrorKind,
}

impl AppError {
    pub fn new(component: Component, kind: io::ErrorKind) -> Self {
        Self { component, kind }
    }

    pub fn from_io(component: Component, error: io::Error) -> Self {
        Self::new(component, error.kind())
    }

    pub fn component(&self) -> Component {
        self.component
    }

    pub fn kind(&self) -> io::ErrorKind {
        self.kind
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.component, self.kind)
    }
}

impl Error for AppError {}

#[cfg(test)]
mod tests {
    use super::{AppError, Component};
    use std::{error::Error, io};

    #[test]
    fn logs_do_not_include_sensitive_content() {
        assert_eq!(log::STATIC_MAX_LEVEL, log::LevelFilter::Off);

        struct Sensitive(&'static str);
        impl std::fmt::Display for Sensitive {
            fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                panic!("sensitive payload must never be formatted")
            }
        }
        for (component, payload) in [
            (Component::Wayland, Sensitive("keyboard=秘密輸入")),
            (Component::Clipboard, Sensitive("clipboard=私人內容")),
            (Component::Pty, Sensitive("pty=密碼輸出")),
        ] {
            log::error!("{payload}");
            log::warn!("{payload}");
            log::info!("{payload}");
            log::debug!("{payload}");
            log::trace!("{payload}");
            let error = AppError::from_io(
                component,
                io::Error::new(io::ErrorKind::InvalidData, payload.0),
            );
            assert_eq!(error.to_string(), format!("{component}: InvalidData"));
            assert!(!format!("{error:?}").contains(payload.0));
            assert!(error.source().is_none());
        }
    }

    #[test]
    fn error_preserves_context_without_secret() {
        let secret = "password=秘密; clipboard=私人內容; key=鍵; pty=敏感輸出";
        let cases = [
            (Component::Wayland, "wayland"),
            (Component::Pty, "pty"),
            (Component::Terminal, "terminal"),
            (Component::Renderer, "renderer"),
            (Component::Clipboard, "clipboard"),
            (Component::Ime, "ime"),
            (Component::Config, "config"),
        ];

        for (component, name) in cases {
            let error = AppError::from_io(
                component,
                io::Error::new(io::ErrorKind::PermissionDenied, secret),
            );
            assert_eq!(error.component(), component);
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert_eq!(error.to_string(), format!("{name}: PermissionDenied"));
            assert_eq!(
                error,
                AppError::new(component, io::ErrorKind::PermissionDenied)
            );
            assert!(error.source().is_none());
            for diagnostic in [
                error.to_string(),
                format!("{error:?}"),
                format!("{error:#?}"),
            ] {
                for sensitive in ["password=", "秘密", "私人內容", "鍵", "敏感輸出"] {
                    assert!(!diagnostic.contains(sensitive));
                }
            }
        }
    }
}
