#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Compositor,
    Shm,
    XdgWmBase,
    Seat,
    Output,
    DataDeviceManager,
    PrimarySelection,
    TextInput,
}

const INTERFACES: [&str; 8] = [
    "wl_compositor",
    "wl_shm",
    "xdg_wm_base",
    "wl_seat",
    "wl_output",
    "wl_data_device_manager",
    "zwp_primary_selection_device_manager_v1",
    "zwp_text_input_manager_v3",
];

#[derive(Debug, Clone, Copy)]
pub struct Global<'a> {
    pub name: u32,
    pub interface: &'a str,
    pub version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub name: u32,
    pub version: u32,
}

/// A bounded snapshot, recomputed from the remaining globals after global_remove.
/// A seat global alone does not imply keyboard or pointer availability.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Capabilities {
    advertised: [Option<Binding>; INTERFACES.len()],
}

impl Capabilities {
    pub fn from_globals(globals: &[Global<'_>]) -> Self {
        let mut capabilities = Self::default();
        for global in globals {
            let Some(index) = INTERFACES.iter().position(|name| *name == global.interface) else {
                continue;
            };
            if global.version == 0 {
                continue;
            }
            let slot = &mut capabilities.advertised[index];
            // Prefer the highest available version, then the lowest global name.
            if slot.is_none_or(|old| {
                global.version > old.version
                    || (global.version == old.version && global.name < old.name)
            }) {
                *slot = Some(Binding {
                    name: global.name,
                    version: global.version,
                });
            }
        }
        capabilities
    }

    /// Plans a bind within the caller's supported range; performs no protocol I/O.
    pub fn binding(&self, protocol: Protocol, minimum: u32, maximum: u32) -> Option<Binding> {
        if minimum == 0 || minimum > maximum {
            return None;
        }
        let advertised = self.advertised[protocol as usize]?;
        let version = advertised.version.min(maximum);
        (version >= minimum).then_some(Binding {
            name: advertised.name,
            version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Binding, Capabilities, Global, Protocol};

    #[test]
    fn primary_selection_capability_is_optional() {
        let clipboard = Global {
            name: 1,
            interface: "wl_data_device_manager",
            version: 3,
        };
        let clipboard_binding = Some(Binding {
            name: 1,
            version: 3,
        });
        let without_primary = Capabilities::from_globals(&[clipboard]);
        assert_eq!(
            without_primary.binding(Protocol::PrimarySelection, 1, 1),
            None
        );
        assert_eq!(
            without_primary.binding(Protocol::DataDeviceManager, 1, 3),
            clipboard_binding
        );

        for version in [0, 1, 2] {
            let primary = Global {
                name: 2,
                interface: "zwp_primary_selection_device_manager_v1",
                version,
            };
            let globals = [clipboard, primary];
            let capabilities = Capabilities::from_globals(&globals);
            assert_eq!(
                capabilities.binding(Protocol::PrimarySelection, 1, 1),
                (version > 0).then_some(Binding {
                    name: 2,
                    version: 1
                })
            );
            assert_eq!(
                capabilities.binding(Protocol::DataDeviceManager, 1, 3),
                clipboard_binding
            );

            let remaining: Vec<_> = globals
                .into_iter()
                .filter(|global| global.name != primary.name)
                .collect();
            assert_eq!(Capabilities::from_globals(&remaining), without_primary);
            assert_eq!(
                Capabilities::from_globals(&[primary]).binding(Protocol::DataDeviceManager, 1, 3),
                None
            );
        }
    }

    #[test]
    fn capabilities_detect_protocols() {
        let globals = [
            Global {
                name: 1,
                interface: "wl_compositor",
                version: 6,
            },
            Global {
                name: 2,
                interface: "xdg_wm_base",
                version: 7,
            },
            Global {
                name: 3,
                interface: "wl_shm",
                version: 1,
            },
            Global {
                name: 4,
                interface: "wl_seat",
                version: 4,
            },
            Global {
                name: 5,
                interface: "wl_seat",
                version: 9,
            },
            Global {
                name: 6,
                interface: "zwp_text_input_manager_v3",
                version: 2,
            },
            Global {
                name: 7,
                interface: "unknown_protocol",
                version: 99,
            },
            Global {
                name: 8,
                interface: "wl_data_device_manager",
                version: 0,
            },
            Global {
                name: 9,
                interface: "wl_seat",
                version: 9,
            },
        ];
        let capabilities = Capabilities::from_globals(&globals);
        assert_eq!(
            capabilities.binding(Protocol::Compositor, 1, 4),
            Some(Binding {
                name: 1,
                version: 4
            })
        );
        assert_eq!(
            capabilities.binding(Protocol::XdgWmBase, 1, 1),
            Some(Binding {
                name: 2,
                version: 1
            })
        );
        assert_eq!(
            capabilities.binding(Protocol::Shm, 1, 1),
            Some(Binding {
                name: 3,
                version: 1
            })
        );
        assert_eq!(
            capabilities.binding(Protocol::Seat, 4, 7),
            Some(Binding {
                name: 5,
                version: 7
            })
        );
        assert_eq!(
            capabilities.binding(Protocol::TextInput, 1, 1),
            Some(Binding {
                name: 6,
                version: 1
            })
        );
        for missing in [
            Protocol::Output,
            Protocol::DataDeviceManager,
            Protocol::PrimarySelection,
        ] {
            assert_eq!(capabilities.binding(missing, 1, 1), None);
        }
        assert_eq!(capabilities.binding(Protocol::Seat, 10, 10), None);
        assert_eq!(capabilities.binding(Protocol::Seat, 2, 1), None);
        assert_eq!(capabilities.binding(Protocol::Seat, 0, 1), None);
        assert_eq!(
            Capabilities::default().binding(Protocol::Compositor, 1, 1),
            None
        );
        let remaining: Vec<_> = globals
            .into_iter()
            .filter(|global| global.name != 5)
            .collect();
        assert_eq!(
            Capabilities::from_globals(&remaining).binding(Protocol::Seat, 1, 20),
            Some(Binding {
                name: 9,
                version: 9
            })
        );
    }
}
