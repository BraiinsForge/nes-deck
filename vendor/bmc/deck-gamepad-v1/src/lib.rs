// Copyright (C) 2026  Braiins Systems s.r.o.

//! Generated client and server bindings for the surface-scoped Deck gamepad
//! protocol. The compositor owns discovery, stable player assignment, hotplug,
//! and focus. Clients retain product-specific button and axis mapping.

/// Server-side protocol bindings for the compositor.
pub mod server {
    #![allow(
        unused_qualifications,
        clippy::all,
        clippy::pedantic,
        missing_debug_implementations,
        reason = "wayland-scanner emits generated bindings outside this crate's lint style"
    )]

    use wayland_server;
    use wayland_server::protocol::*;

    pub mod __interfaces {
        use wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocol/deck-gamepad-v1.xml");
    }

    use self::__interfaces::*;

    wayland_scanner::generate_server_code!("./protocol/deck-gamepad-v1.xml");
}

/// Client-side protocol bindings for native applications and widgets.
pub mod client {
    #![allow(
        unused_qualifications,
        clippy::all,
        clippy::pedantic,
        missing_debug_implementations,
        reason = "wayland-scanner emits generated bindings outside this crate's lint style"
    )]

    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocol/deck-gamepad-v1.xml");
    }

    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./protocol/deck-gamepad-v1.xml");
}
