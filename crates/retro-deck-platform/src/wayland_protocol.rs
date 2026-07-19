//! Generated client bindings for the BMC Deck widget protocol.

/// Version 1 client API generated from the checked-in wire contract.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    unused_imports,
    unused_unsafe,
    unused_variables,
    clippy::all,
    clippy::pedantic,
    reason = "the public API is mechanically generated from Wayland XML"
)]
pub mod deck_widget_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    #[doc(hidden)]
    pub mod __interfaces {
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;

        wayland_scanner::generate_interfaces!("../../protocol/deck-widget-v1.xml");
    }

    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("../../protocol/deck-widget-v1.xml");
}
