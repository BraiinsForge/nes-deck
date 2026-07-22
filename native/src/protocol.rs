pub mod deck_widget {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("../protocol/deck-widget-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("../protocol/deck-widget-v1.xml");
}
