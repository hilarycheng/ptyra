use crate::{AppEffect, AppEvent, EventSource, config::Config, reduce};

fn reviewed_dependencies(manifest: &toml::Table) -> bool {
    fn check(deps: &toml::Value) -> bool {
        deps.as_table().is_some_and(|deps| {
            deps.iter().all(|(name, spec)| {
                let (version, allowed): (&str, &[&str]) = match name.as_str() {
                    "alacritty_terminal" => ("=0.26.0", &[]),
                    "kbvm" => ("=0.2.0", &[]),
                    "log" => ("=0.4.34", &["max_level_off", "release_max_level_off"]),
                    "toml" => ("=1.1.6", &["std", "parse", "serde"]),
                    "nix" => ("=0.31.3", &["fs", "process", "signal", "term"]),
                    _ => return false,
                };
                let Some(spec) = spec.as_table() else {
                    return false;
                };
                spec.keys()
                    .all(|key| matches!(key.as_str(), "version" | "default-features" | "features"))
                    && spec.get("version").and_then(toml::Value::as_str) == Some(version)
                    && spec.get("default-features").and_then(toml::Value::as_bool) == Some(false)
                    && spec.get("features").is_none_or(|features| {
                        features.as_array().is_some_and(|features| {
                            features.iter().all(|feature| {
                                feature
                                    .as_str()
                                    .is_some_and(|feature| allowed.contains(&feature))
                            })
                        })
                    })
            })
        })
    }
    manifest.get("dependencies").is_some_and(check)
        && manifest.get("target").is_none_or(|targets| {
            targets.as_table().is_some_and(|targets| {
                targets
                    .values()
                    .all(|target| target.get("dependencies").is_none_or(check))
            })
        })
}

#[test]
fn app_telemetry_is_disabled_by_design() {
    let manifest: toml::Table = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    assert!(reviewed_dependencies(&manifest));
    let mut added = manifest.clone();
    added["dependencies"]
        .as_table_mut()
        .unwrap()
        .insert("reqwest".into(), "*".into());
    assert!(!reviewed_dependencies(&added));
    let mut feature = manifest;
    feature["dependencies"]["kbvm"]
        .as_table_mut()
        .unwrap()
        .insert(
            "features".into(),
            toml::Value::Array(vec!["unreviewed-network-feature".into()]),
        );
    assert!(!reviewed_dependencies(&feature));
    for source in [
        "telemetry = true",
        "analytics = true",
        "update_url = 'https://example.test'",
    ] {
        assert!(Config::from_toml(Some(source)).is_err());
    }
    // Exhaustive matches force review when the app gains another source or effect.
    fn local_effect(effect: AppEffect) {
        match effect {
            AppEffect::Route(source) => match source {
                EventSource::Wayland
                | EventSource::Pty
                | EventSource::Ime
                | EventSource::Clipboard => {}
            },
            AppEffect::StopRendering(_) | AppEffect::PtyExited { .. } => {}
        }
    }
    for event in [
        AppEvent::Wayland,
        AppEvent::Pty,
        AppEvent::Ime,
        AppEvent::Clipboard,
        AppEvent::WaylandDisconnected(std::io::ErrorKind::BrokenPipe),
    ] {
        local_effect(reduce(&event));
    }
}
