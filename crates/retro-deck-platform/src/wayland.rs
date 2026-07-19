//! BMC widget and gameplay surfaces driven through the pure Rust Wayland client.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use wayland_client::backend::WaylandError as TransportError;
use wayland_client::globals::{BindError, GlobalError, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_compositor, wl_region, wl_registry, wl_seat, wl_surface, wl_touch,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use crate::display::{DECK_DIMENSIONS, Dimensions, DisplayError, gameplay_dimensions};
use crate::wayland_protocol::deck_widget_v1::{deck_widget_manager_v1, deck_widget_surface_v1};

const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(2);
const CONFIGURE_POLL_SLICE: Duration = Duration::from_millis(100);
const MAXIMUM_TOUCH_REPORTS: usize = 64;

/// One bounded touch update delivered by the BMC widget seat.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TouchReport {
    x: u16,
    y: u16,
    down: bool,
    pressed: bool,
    released: bool,
}

impl TouchReport {
    /// Horizontal surface coordinate.
    #[must_use]
    pub const fn x(self) -> u16 {
        self.x
    }

    /// Vertical surface coordinate.
    #[must_use]
    pub const fn y(self) -> u16 {
        self.y
    }

    /// Whether the primary contact remains down.
    #[must_use]
    pub const fn down(self) -> bool {
        self.down
    }

    /// Whether this report began the primary contact.
    #[must_use]
    pub const fn pressed(self) -> bool {
        self.pressed
    }

    /// Whether this report ended or cancelled the primary contact.
    #[must_use]
    pub const fn released(self) -> bool {
        self.released
    }
}

#[derive(Debug)]
struct TouchQueue {
    reports: VecDeque<TouchReport>,
    dropped: usize,
}

impl TouchQueue {
    fn new() -> Self {
        Self {
            reports: VecDeque::with_capacity(8),
            dropped: 0,
        }
    }

    fn push(&mut self, report: TouchReport) {
        if !report.pressed && !report.released {
            if let Some(previous) = self.reports.back_mut() {
                if !previous.pressed && !previous.released {
                    *previous = report;
                    return;
                }
            }
        }
        if self.reports.len() == MAXIMUM_TOUCH_REPORTS {
            let _ = self.reports.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.reports.push_back(report);
    }

    fn take(&mut self) -> (Vec<TouchReport>, usize) {
        let reports = self.reports.drain(..).collect();
        let dropped = std::mem::take(&mut self.dropped);
        (reports, dropped)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LayerRole {
    Game,
    Background,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigureState {
    AwaitingWidgetConfigure,
    AwaitingWidgetDone,
    AwaitingGameplay { game: bool, background: bool },
    Ready,
    Invalid,
    Closed,
}

impl ConfigureState {
    const fn widget() -> Self {
        Self::AwaitingWidgetConfigure
    }

    const fn gameplay() -> Self {
        Self::AwaitingGameplay {
            game: false,
            background: false,
        }
    }

    const fn widget_dimensions(&mut self) {
        match self {
            Self::AwaitingWidgetConfigure => *self = Self::AwaitingWidgetDone,
            Self::AwaitingWidgetDone | Self::Ready => {}
            Self::AwaitingGameplay { .. } | Self::Invalid | Self::Closed => {
                *self = Self::Invalid;
            }
        }
    }

    const fn widget_done(&mut self) {
        *self = match self {
            Self::AwaitingWidgetDone | Self::Ready => Self::Ready,
            Self::AwaitingWidgetConfigure
            | Self::AwaitingGameplay { .. }
            | Self::Invalid
            | Self::Closed => Self::Invalid,
        };
    }

    const fn layer_configured(&mut self, role: LayerRole) {
        match self {
            Self::AwaitingGameplay { game, background } => match role {
                LayerRole::Game => *game = true,
                LayerRole::Background => *background = true,
            },
            Self::Ready => return,
            Self::AwaitingWidgetConfigure
            | Self::AwaitingWidgetDone
            | Self::Invalid
            | Self::Closed => {
                *self = Self::Invalid;
                return;
            }
        }
        if matches!(
            self,
            Self::AwaitingGameplay {
                game: true,
                background: true
            }
        ) {
            *self = Self::Ready;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Visibility {
    Visible,
    Hidden,
}

#[derive(Debug)]
struct EventState {
    main_surface: wl_surface::WlSurface,
    dimensions: Dimensions,
    configure: ConfigureState,
    visibility: Visibility,
    touch: Option<wl_touch::WlTouch>,
    primary_touch: Option<i32>,
    touch_x: u16,
    touch_y: u16,
    touch_reports: TouchQueue,
}

impl EventState {
    fn new(
        main_surface: wl_surface::WlSurface,
        dimensions: Dimensions,
        require_background: bool,
    ) -> Self {
        Self {
            main_surface,
            dimensions,
            configure: if require_background {
                ConfigureState::gameplay()
            } else {
                ConfigureState::widget()
            },
            visibility: Visibility::Visible,
            touch: None,
            primary_touch: None,
            touch_x: 0,
            touch_y: 0,
            touch_reports: TouchQueue::new(),
        }
    }

    const fn ready(&self) -> bool {
        matches!(self.configure, ConfigureState::Ready)
    }

    const fn invalid_configure(&self) -> bool {
        matches!(self.configure, ConfigureState::Invalid)
    }

    const fn shutdown(&self) -> bool {
        matches!(self.configure, ConfigureState::Closed)
    }

    fn apply_widget_configure(&mut self, width: u32, height: u32) {
        let Some(dimensions) = dimensions_from_protocol(width, height) else {
            self.configure = ConfigureState::Invalid;
            return;
        };
        self.dimensions = dimensions;
        self.configure.widget_dimensions();
    }

    fn apply_layer_configure(&mut self, role: LayerRole, width: u32, height: u32) {
        if matches!(
            self.configure,
            ConfigureState::Closed | ConfigureState::Invalid
        ) {
            return;
        }
        match role {
            LayerRole::Background => {}
            LayerRole::Game => {
                if width != 0 || height != 0 {
                    let Some(dimensions) = dimensions_from_protocol(width, height) else {
                        self.configure = ConfigureState::Invalid;
                        return;
                    };
                    self.dimensions = dimensions;
                }
            }
        }
        self.configure.layer_configured(role);
    }

    fn update_touch_point(&mut self, x: f64, y: f64) {
        self.touch_x = clamp_coordinate(x, self.dimensions.width());
        self.touch_y = clamp_coordinate(y, self.dimensions.height());
    }

    fn push_touch(&mut self, pressed: bool, released: bool) {
        let report = TouchReport {
            x: self.touch_x,
            y: self.touch_y,
            down: self.primary_touch.is_some(),
            pressed,
            released,
        };
        self.touch_reports.push(report);
    }

    fn cancel_touch(&mut self) {
        if self.primary_touch.take().is_some() {
            self.push_touch(false, true);
        }
    }
}

fn dimensions_from_protocol(width: u32, height: u32) -> Option<Dimensions> {
    let width = usize::try_from(width).ok()?;
    let height = usize::try_from(height).ok()?;
    Dimensions::new(width, height)
}

fn clamp_coordinate(value: f64, extent: usize) -> u16 {
    let maximum = extent.saturating_sub(1).min(usize::from(u16::MAX));
    let maximum = u16::try_from(maximum).unwrap_or(u16::MAX);
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    if value >= f64::from(maximum) {
        return maximum;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "finite positive Wayland coordinates are clamped below u16::MAX"
    )]
    let coordinate = value as u16;
    coordinate
}

enum SurfaceObjects {
    Widget {
        widget_surface: deck_widget_surface_v1::DeckWidgetSurfaceV1,
    },
    Gameplay {
        game_layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        background_surface: wl_surface::WlSurface,
        background_layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    },
}

impl fmt::Debug for SurfaceObjects {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Widget { .. } => formatter.write_str("Widget"),
            Self::Gameplay { .. } => formatter.write_str("Gameplay"),
        }
    }
}

/// Configured BMC widget or gameplay surface without presentation buffers.
///
/// Buffer allocation and frame attachment are owned by the presentation layer;
/// this type owns protocol event dispatch, lifecycle, and widget touch input.
#[derive(Debug)]
pub struct WaylandSurface {
    event_queue: EventQueue<EventState>,
    state: EventState,
    main_surface: wl_surface::WlSurface,
    objects: SurfaceObjects,
    _compositor: wl_compositor::WlCompositor,
    _seat: Option<wl_seat::WlSeat>,
}

impl WaylandSurface {
    /// Connect to BMC and create a swipeable dashboard widget.
    ///
    /// # Errors
    ///
    /// Returns [`WaylandSurfaceError`] when the compositor, required global,
    /// surface role, or initial two-second configure handshake fails.
    pub fn connect_widget() -> Result<Self, WaylandSurfaceError> {
        let connection = Connection::connect_to_env().map_err(WaylandSurfaceError::Connect)?;
        let (globals, event_queue) =
            registry_queue_init::<EventState>(&connection).map_err(WaylandSurfaceError::Globals)?;
        let handle = event_queue.handle();
        let compositor = bind_required::<wl_compositor::WlCompositor, _>(
            &globals,
            &handle,
            1..=4,
            (),
            "wl_compositor",
        )?;
        let manager = bind_required::<deck_widget_manager_v1::DeckWidgetManagerV1, _>(
            &globals,
            &handle,
            1..=1,
            (),
            "deck_widget_manager_v1",
        )?;
        let seat = bind_optional::<wl_seat::WlSeat, _>(&globals, &handle, 1..=7, ());
        let main_surface = compositor.create_surface(&handle, ());
        let widget_surface = manager.get_widget_surface(&main_surface, &handle, ());
        let state = EventState::new(main_surface.clone(), DECK_DIMENSIONS, false);
        main_surface.commit();

        let mut surface = Self {
            event_queue,
            state,
            main_surface,
            objects: SurfaceObjects::Widget { widget_surface },
            _compositor: compositor,
            _seat: seat,
        };
        surface.wait_until_configured()?;
        Ok(surface)
    }

    /// Connect to BMC and create a centered gameplay layer over black.
    ///
    /// # Errors
    ///
    /// Returns [`WaylandSurfaceError`] when dimensions do not fit, the
    /// compositor lacks layer-shell, or either surface misses its configure
    /// deadline.
    pub fn connect_gameplay(source: Dimensions) -> Result<Self, WaylandSurfaceError> {
        let dimensions = gameplay_dimensions(source).map_err(WaylandSurfaceError::Display)?;
        let connection = Connection::connect_to_env().map_err(WaylandSurfaceError::Connect)?;
        let (globals, event_queue) =
            registry_queue_init::<EventState>(&connection).map_err(WaylandSurfaceError::Globals)?;
        let handle = event_queue.handle();
        let compositor = bind_required::<wl_compositor::WlCompositor, _>(
            &globals,
            &handle,
            1..=4,
            (),
            "wl_compositor",
        )?;
        let layer_shell = bind_required::<zwlr_layer_shell_v1::ZwlrLayerShellV1, _>(
            &globals,
            &handle,
            1..=4,
            (),
            "zwlr_layer_shell_v1",
        )?;

        let empty_region = compositor.create_region(&handle, ());
        let background_surface = compositor.create_surface(&handle, ());
        background_surface.set_input_region(Some(&empty_region));
        let background_layer = layer_shell.get_layer_surface(
            &background_surface,
            None,
            zwlr_layer_shell_v1::Layer::Overlay,
            "retro-deck-game-background".to_owned(),
            &handle,
            LayerRole::Background,
        );
        background_layer.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        background_layer.set_size(0, 0);
        background_layer.set_exclusive_zone(-1);
        background_layer
            .set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        background_surface.commit();

        let main_surface = compositor.create_surface(&handle, ());
        main_surface.set_input_region(Some(&empty_region));
        empty_region.destroy();
        let game_layer = layer_shell.get_layer_surface(
            &main_surface,
            None,
            zwlr_layer_shell_v1::Layer::Overlay,
            "retro-deck-game".to_owned(),
            &handle,
            LayerRole::Game,
        );
        game_layer.set_size(
            u32::try_from(dimensions.width()).map_err(|_| WaylandSurfaceError::InvalidSize)?,
            u32::try_from(dimensions.height()).map_err(|_| WaylandSurfaceError::InvalidSize)?,
        );
        game_layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        main_surface.commit();

        let state = EventState::new(main_surface.clone(), dimensions, true);
        let mut surface = Self {
            event_queue,
            state,
            main_surface,
            objects: SurfaceObjects::Gameplay {
                game_layer,
                background_surface,
                background_layer,
            },
            _compositor: compositor,
            _seat: None,
        };
        surface.wait_until_configured()?;
        Ok(surface)
    }

    /// Configured surface dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> Dimensions {
        self.state.dimensions
    }

    /// Whether BMC currently considers the dashboard widget visible.
    #[must_use]
    pub const fn visible(&self) -> bool {
        matches!(self.state.visibility, Visibility::Visible)
    }

    /// Whether the compositor requested permanent surface shutdown.
    #[must_use]
    pub const fn shutdown_requested(&self) -> bool {
        self.state.shutdown()
    }

    /// Drain bounded widget touch reports and return the number previously
    /// dropped because the consumer fell behind.
    pub fn take_touch_reports(&mut self) -> (Vec<TouchReport>, usize) {
        self.state.touch_reports.take()
    }

    /// Read and dispatch any Wayland events currently available without
    /// waiting for the compositor.
    ///
    /// # Errors
    ///
    /// Returns [`WaylandSurfaceError`] on transport, protocol, or invalid
    /// configure state.
    pub fn dispatch_nonblocking(&mut self) -> Result<usize, WaylandSurfaceError> {
        self.drive(Some(Duration::ZERO))
    }

    fn wait_until_configured(&mut self) -> Result<(), WaylandSurfaceError> {
        let deadline = Instant::now() + CONFIGURE_TIMEOUT;
        while !self.state.ready() && !self.state.shutdown() {
            if self.state.invalid_configure() {
                return Err(WaylandSurfaceError::InvalidConfigure);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(WaylandSurfaceError::ConfigureTimeout);
            }
            let timeout = (deadline - now).min(CONFIGURE_POLL_SLICE);
            self.drive(Some(timeout))?;
        }
        if self.state.shutdown() {
            return Err(WaylandSurfaceError::ClosedDuringConfigure);
        }
        if self.state.invalid_configure() {
            return Err(WaylandSurfaceError::InvalidConfigure);
        }
        Ok(())
    }

    fn drive(&mut self, timeout: Option<Duration>) -> Result<usize, WaylandSurfaceError> {
        let mut dispatched = self
            .event_queue
            .dispatch_pending(&mut self.state)
            .map_err(WaylandSurfaceError::Dispatch)?;
        flush_allowing_backpressure(&self.event_queue)?;

        let Some(guard) = self.event_queue.prepare_read() else {
            dispatched += self
                .event_queue
                .dispatch_pending(&mut self.state)
                .map_err(WaylandSurfaceError::Dispatch)?;
            return self.validate_after_dispatch(dispatched);
        };
        let mut descriptors = [PollFd::from_borrowed_fd(
            guard.connection_fd(),
            PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
        )];
        let timeout = timeout.map(duration_timespec);
        let ready = match poll(&mut descriptors, timeout.as_ref()) {
            Ok(ready) => ready,
            Err(rustix::io::Errno::INTR) => 0,
            Err(source) => return Err(WaylandSurfaceError::Poll(source)),
        };
        if ready > 0 {
            match guard.read() {
                Ok(_) => {}
                Err(TransportError::Io(source)) if source.kind() == io::ErrorKind::WouldBlock => {}
                Err(source) => return Err(WaylandSurfaceError::Transport(source)),
            }
        } else {
            drop(guard);
        }
        dispatched += self
            .event_queue
            .dispatch_pending(&mut self.state)
            .map_err(WaylandSurfaceError::Dispatch)?;
        self.validate_after_dispatch(dispatched)
    }

    const fn validate_after_dispatch(
        &self,
        dispatched: usize,
    ) -> Result<usize, WaylandSurfaceError> {
        if self.state.invalid_configure() {
            Err(WaylandSurfaceError::InvalidConfigure)
        } else {
            Ok(dispatched)
        }
    }
}

impl AsFd for WaylandSurface {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.event_queue.as_fd()
    }
}

impl Drop for WaylandSurface {
    fn drop(&mut self) {
        match &self.objects {
            SurfaceObjects::Widget { widget_surface } => widget_surface.destroy(),
            SurfaceObjects::Gameplay {
                game_layer,
                background_surface,
                background_layer,
            } => {
                game_layer.destroy();
                background_layer.destroy();
                background_surface.destroy();
            }
        }
        self.main_surface.destroy();
    }
}

fn duration_timespec(duration: Duration) -> Timespec {
    Timespec {
        tv_sec: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(duration.subsec_nanos()),
    }
}

fn flush_allowing_backpressure(
    event_queue: &EventQueue<EventState>,
) -> Result<(), WaylandSurfaceError> {
    match event_queue.flush() {
        Ok(()) => Ok(()),
        Err(TransportError::Io(source)) if source.kind() == io::ErrorKind::WouldBlock => Ok(()),
        Err(source) => Err(WaylandSurfaceError::Transport(source)),
    }
}

fn bind_required<I, U>(
    globals: &wayland_client::globals::GlobalList,
    handle: &QueueHandle<EventState>,
    version: std::ops::RangeInclusive<u32>,
    data: U,
    interface: &'static str,
) -> Result<I, WaylandSurfaceError>
where
    I: Proxy + 'static,
    EventState: Dispatch<I, U>,
    U: Send + Sync + 'static,
{
    globals
        .bind(handle, version, data)
        .map_err(|source| WaylandSurfaceError::Bind { interface, source })
}

fn bind_optional<I, U>(
    globals: &wayland_client::globals::GlobalList,
    handle: &QueueHandle<EventState>,
    version: std::ops::RangeInclusive<u32>,
    data: U,
) -> Option<I>
where
    I: Proxy + 'static,
    EventState: Dispatch<I, U>,
    U: Send + Sync + 'static,
{
    globals.bind(handle, version, data).ok()
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for EventState {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(EventState: ignore wl_compositor::WlCompositor);
delegate_noop!(EventState: ignore wl_region::WlRegion);
delegate_noop!(EventState: ignore wl_surface::WlSurface);

impl Dispatch<deck_widget_manager_v1::DeckWidgetManagerV1, ()> for EventState {
    fn event(
        _state: &mut Self,
        _manager: &deck_widget_manager_v1::DeckWidgetManagerV1,
        _event: deck_widget_manager_v1::Event,
        _data: &(),
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<deck_widget_surface_v1::DeckWidgetSurfaceV1, ()> for EventState {
    fn event(
        state: &mut Self,
        _surface: &deck_widget_surface_v1::DeckWidgetSurfaceV1,
        event: deck_widget_surface_v1::Event,
        _data: &(),
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        match event {
            deck_widget_surface_v1::Event::Configure { width, height, .. } => {
                state.apply_widget_configure(width, height);
            }
            deck_widget_surface_v1::Event::ConfigureDone => {
                state.configure.widget_done();
            }
            deck_widget_surface_v1::Event::Lifecycle { state: lifecycle } => {
                state.visibility = if lifecycle == 0 {
                    Visibility::Hidden
                } else {
                    Visibility::Visible
                };
            }
            deck_widget_surface_v1::Event::Shutdown => {
                state.configure = ConfigureState::Closed;
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for EventState {
    fn event(
        _state: &mut Self,
        _layer_shell: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _event: zwlr_layer_shell_v1::Event,
        _data: &(),
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, LayerRole> for EventState {
    fn event(
        state: &mut Self,
        surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        role: &LayerRole,
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                surface.ack_configure(serial);
                state.apply_layer_configure(*role, width, height);
            }
            zwlr_layer_surface_v1::Event::Closed => {
                state.configure = ConfigureState::Closed;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for EventState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _connection: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities { capabilities } = event else {
            return;
        };
        let WEnum::Value(capabilities) = capabilities else {
            return;
        };
        let have_touch = capabilities.contains(wl_seat::Capability::Touch);
        if have_touch && state.touch.is_none() {
            state.touch = Some(seat.get_touch(handle, ()));
        } else if !have_touch {
            state.cancel_touch();
            if let Some(touch) = state.touch.take() {
                if touch.version() >= 3 {
                    touch.release();
                }
            }
        }
    }
}

impl Dispatch<wl_touch::WlTouch, ()> for EventState {
    fn event(
        state: &mut Self,
        _touch: &wl_touch::WlTouch,
        event: wl_touch::Event,
        _data: &(),
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        match event {
            wl_touch::Event::Down {
                surface, id, x, y, ..
            } if surface == state.main_surface && state.primary_touch.is_none() => {
                state.primary_touch = Some(id);
                state.update_touch_point(x, y);
                state.push_touch(true, false);
            }
            wl_touch::Event::Motion { id, x, y, .. } if state.primary_touch == Some(id) => {
                state.update_touch_point(x, y);
                state.push_touch(false, false);
            }
            wl_touch::Event::Up { id, .. } if state.primary_touch == Some(id) => {
                state.primary_touch = None;
                state.push_touch(false, true);
            }
            wl_touch::Event::Cancel => state.cancel_touch(),
            _ => {}
        }
    }
}

/// Wayland connection, protocol, polling, or configure failure.
#[derive(Debug)]
pub enum WaylandSurfaceError {
    /// Environment did not provide a reachable compositor socket.
    Connect(wayland_client::ConnectError),
    /// Initial global discovery failed.
    Globals(GlobalError),
    /// A required global was missing or too old.
    Bind {
        /// Required protocol interface.
        interface: &'static str,
        /// Binding failure.
        source: BindError,
    },
    /// Event dispatch failed.
    Dispatch(wayland_client::DispatchError),
    /// Wayland transport failed.
    Transport(TransportError),
    /// Native descriptor polling failed.
    Poll(rustix::io::Errno),
    /// Source dimensions cannot produce a gameplay surface.
    Display(DisplayError),
    /// Configured dimensions cannot be represented by the wire protocol.
    InvalidSize,
    /// The compositor sent zero or excessive dimensions.
    InvalidConfigure,
    /// The initial configure handshake exceeded two seconds.
    ConfigureTimeout,
    /// The compositor closed a surface during setup.
    ClosedDuringConfigure,
}

impl fmt::Display for WaylandSurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(source) => write!(formatter, "cannot connect to Wayland: {source}"),
            Self::Globals(source) => write!(formatter, "cannot discover Wayland globals: {source}"),
            Self::Bind { interface, source } => {
                write!(formatter, "cannot bind {interface}: {source}")
            }
            Self::Dispatch(source) => write!(formatter, "Wayland dispatch failed: {source}"),
            Self::Transport(source) => write!(formatter, "Wayland transport failed: {source}"),
            Self::Poll(source) => write!(formatter, "Wayland poll failed: {source}"),
            Self::Display(source) => write!(formatter, "invalid gameplay display: {source}"),
            Self::InvalidSize => formatter.write_str("Wayland surface size is not representable"),
            Self::InvalidConfigure => {
                formatter.write_str("compositor sent invalid surface dimensions")
            }
            Self::ConfigureTimeout => {
                formatter.write_str("timed out awaiting Wayland surface configure")
            }
            Self::ClosedDuringConfigure => {
                formatter.write_str("Wayland surface closed during configure")
            }
        }
    }
}

impl Error for WaylandSurfaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Connect(source) => Some(source),
            Self::Globals(source) => Some(source),
            Self::Bind { source, .. } => Some(source),
            Self::Dispatch(source) => Some(source),
            Self::Transport(source) => Some(source),
            Self::Poll(source) => Some(source),
            Self::Display(source) => Some(source),
            Self::InvalidSize
            | Self::InvalidConfigure
            | Self::ConfigureTimeout
            | Self::ClosedDuringConfigure => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_dimensions_are_checked() {
        assert_eq!(dimensions_from_protocol(0, 480), None);
        assert_eq!(dimensions_from_protocol(1_280, 0), None);
        assert_eq!(dimensions_from_protocol(1_280, 480), Some(DECK_DIMENSIONS));
        assert_eq!(dimensions_from_protocol(20_000, 480), None);
    }

    #[test]
    fn widget_requires_dimensions_before_configure_done() {
        let mut configure = ConfigureState::widget();
        configure.widget_done();
        assert_eq!(configure, ConfigureState::Invalid);

        let mut configure = ConfigureState::widget();
        configure.widget_dimensions();
        assert_eq!(configure, ConfigureState::AwaitingWidgetDone);
        configure.widget_done();
        assert_eq!(configure, ConfigureState::Ready);
    }

    #[test]
    fn gameplay_requires_both_layer_configures_in_either_order() {
        let mut background_first = ConfigureState::gameplay();
        background_first.layer_configured(LayerRole::Background);
        assert_ne!(background_first, ConfigureState::Ready);
        background_first.layer_configured(LayerRole::Game);
        assert_eq!(background_first, ConfigureState::Ready);

        let mut game_first = ConfigureState::gameplay();
        game_first.layer_configured(LayerRole::Game);
        assert_ne!(game_first, ConfigureState::Ready);
        game_first.layer_configured(LayerRole::Background);
        assert_eq!(game_first, ConfigureState::Ready);
    }

    #[test]
    fn wrong_surface_role_invalidates_the_handshake() {
        let mut widget = ConfigureState::widget();
        widget.layer_configured(LayerRole::Game);
        assert_eq!(widget, ConfigureState::Invalid);

        let mut gameplay = ConfigureState::gameplay();
        gameplay.widget_dimensions();
        assert_eq!(gameplay, ConfigureState::Invalid);
    }

    #[test]
    fn touch_motion_is_coalesced_without_losing_edges() {
        let mut queue = TouchQueue::new();
        queue.push(TouchReport {
            x: 1,
            y: 2,
            down: true,
            pressed: true,
            released: false,
        });
        for x in 3..40 {
            queue.push(TouchReport {
                x,
                y: 4,
                down: true,
                pressed: false,
                released: false,
            });
        }
        queue.push(TouchReport {
            x: 39,
            y: 4,
            down: false,
            pressed: false,
            released: true,
        });
        let (reports, dropped) = queue.take();
        assert_eq!(dropped, 0);
        assert_eq!(reports.len(), 3);
        assert!(reports.first().is_some_and(|report| report.pressed));
        assert!(reports.get(1).is_some_and(|report| report.x == 39));
        assert!(reports.last().is_some_and(|report| report.released));
    }

    #[test]
    fn bounded_touch_queue_preserves_the_newest_release() {
        let mut queue = TouchQueue::new();
        for x in 0..MAXIMUM_TOUCH_REPORTS {
            queue.push(TouchReport {
                x: u16::try_from(x).unwrap_or_default(),
                y: 0,
                down: true,
                pressed: true,
                released: false,
            });
        }
        queue.push(TouchReport {
            x: 100,
            y: 0,
            down: false,
            pressed: false,
            released: true,
        });
        let (reports, dropped) = queue.take();
        assert_eq!(reports.len(), MAXIMUM_TOUCH_REPORTS);
        assert_eq!(dropped, 1);
        assert!(reports.last().is_some_and(|report| report.released));
        let (_, dropped_after_take) = queue.take();
        assert_eq!(dropped_after_take, 0);
    }

    #[test]
    fn floating_touch_coordinates_are_finite_and_clamped() {
        assert_eq!(clamp_coordinate(f64::NAN, 1_280), 0);
        assert_eq!(clamp_coordinate(-1.0, 1_280), 0);
        assert_eq!(clamp_coordinate(17.9, 1_280), 17);
        assert_eq!(clamp_coordinate(1_280.0, 1_280), 1_279);
        assert_eq!(clamp_coordinate(f64::INFINITY, 1_280), 0);
    }

    #[test]
    fn poll_timeout_conversion_is_exact_and_saturating() {
        assert_eq!(
            duration_timespec(Duration::from_millis(125)),
            Timespec {
                tv_sec: 0,
                tv_nsec: 125_000_000,
            }
        );
        assert_eq!(
            duration_timespec(Duration::MAX).tv_sec,
            i64::try_from(Duration::MAX.as_secs()).unwrap_or(i64::MAX)
        );
    }
}
