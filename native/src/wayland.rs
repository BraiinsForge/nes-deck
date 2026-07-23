use crate::input::TouchReport;
use crate::protocol::deck_widget::{deck_widget_manager_v1, deck_widget_surface_v1};
use crate::{canvas, controls, polling};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use wayland_client::backend::WaylandError;
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, wl_touch,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop};

const BUFFER_COUNT: usize = 3;
const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(2);
const O_CLOEXEC: i32 = 0o2000000;
static SHM_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Mapping {
    pointer: *mut c_void,
    size: usize,
}

impl Mapping {
    fn new(size: usize) -> Result<(Self, OwnedFd), String> {
        let fd = anonymous_file()?;
        ftruncate(&fd, size as u64)
            .map_err(|error| format!("cannot size Wayland shared memory file: {error}"))?;
        let pointer = unsafe {
            mmap(
                ptr::null_mut(),
                size,
                ProtFlags::READ | ProtFlags::WRITE,
                MapFlags::SHARED,
                &fd,
                0,
            )
        }
        .map_err(|error| format!("cannot map Wayland frame buffer: {error}"))?;
        Ok((Self { pointer, size }, fd))
    }

    fn pixels(&mut self) -> &mut [u32] {
        unsafe { slice::from_raw_parts_mut(self.pointer.cast(), self.size / 4) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            let _ = munmap(self.pointer, self.size);
        }
    }
}

fn anonymous_file() -> Result<OwnedFd, String> {
    if let Ok(fd) = memfd_create("retro-deck-wayland", MemfdFlags::CLOEXEC) {
        return Ok(fd);
    }
    let directory = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    for _ in 0..100 {
        let sequence = SHM_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            "retro-deck-wayland-{}-{sequence}",
            std::process::id()
        ));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(O_CLOEXEC)
            .open(&path)
        {
            Ok(file) => {
                std::fs::remove_file(&path).map_err(|error| {
                    format!("cannot unlink Wayland shared memory file: {error}")
                })?;
                return Ok(file.into());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!("cannot create Wayland shared memory file: {error}"));
            }
        }
    }
    Err("cannot create a unique Wayland shared memory file".to_owned())
}

struct BufferSlot {
    mapping: Mapping,
    buffer: wl_buffer::WlBuffer,
    busy: bool,
}

#[derive(Default)]
struct State {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    seat: Option<wl_seat::WlSeat>,
    touch: Option<wl_touch::WlTouch>,
    manager: Option<deck_widget_manager_v1::DeckWidgetManagerV1>,
    widget_surface: Option<deck_widget_surface_v1::DeckWidgetSurfaceV1>,
    surface: Option<wl_surface::WlSurface>,
    configured: bool,
    width: u32,
    height: u32,
    visible: bool,
    shutdown: bool,
    touch_x: i32,
    touch_y: i32,
    touch_down: bool,
    touches: VecDeque<TouchReport>,
    slots: Vec<BufferSlot>,
}

impl State {
    fn push_touch(&mut self, pressed: bool, released: bool) {
        let max_x = self.width.saturating_sub(1) as i32;
        let max_y = self.height.saturating_sub(1) as i32;
        self.touches.push_back(TouchReport {
            x: self.touch_x.clamp(0, max_x),
            y: self.touch_y.clamp(0, max_y),
            down: self.touch_down,
            pressed,
            released,
        });
    }
}

struct Widget {
    queue: EventQueue<State>,
    state: State,
}

impl Widget {
    fn open() -> Result<Self, String> {
        let connection = Connection::connect_to_env()
            .map_err(|error| format!("cannot connect to the Wayland display: {error}"))?;
        let mut queue = connection.new_event_queue::<State>();
        let qh = queue.handle();
        connection.display().get_registry(&qh, ());
        let mut state = State {
            visible: true,
            ..State::default()
        };
        queue
            .roundtrip(&mut state)
            .map_err(|error| format!("cannot bind Wayland globals: {error}"))?;

        let compositor = state
            .compositor
            .clone()
            .ok_or_else(|| "Wayland compositor global is unavailable".to_owned())?;
        state
            .shm
            .as_ref()
            .ok_or_else(|| "Wayland shared memory global is unavailable".to_owned())?;
        let manager = state
            .manager
            .clone()
            .ok_or_else(|| "Deck widget protocol is unavailable".to_owned())?;
        let surface = compositor.create_surface(&qh, ());
        let widget_surface = manager.get_widget_surface(&surface, &qh, ());
        state.surface = Some(surface.clone());
        state.widget_surface = Some(widget_surface);
        surface.commit();

        let mut widget = Self { queue, state };
        let deadline = Instant::now() + CONFIGURE_TIMEOUT;
        while !widget.state.configured && !widget.state.shutdown {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || widget.dispatch(duration_ms(remaining))? == 0 {
                return Err("timed out awaiting Wayland surface configure".to_owned());
            }
        }
        if widget.state.shutdown {
            return Err("Wayland surface was closed during configure".to_owned());
        }
        Ok(widget)
    }

    fn dispatch(&mut self, timeout_ms: u32) -> Result<usize, String> {
        let dispatched = self
            .queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| format!("cannot dispatch Wayland events: {error}"))?;
        if dispatched > 0 {
            return Ok(dispatched);
        }
        self.flush("cannot flush Wayland display")?;

        let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
        loop {
            let Some(guard) = self.queue.prepare_read() else {
                let dispatched = self
                    .queue
                    .dispatch_pending(&mut self.state)
                    .map_err(|error| format!("cannot dispatch Wayland events: {error}"))?;
                if dispatched > 0 {
                    return Ok(dispatched);
                }
                if Instant::now() >= deadline {
                    return Ok(0);
                }
                continue;
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            let timeout = Timespec {
                tv_sec: remaining.as_secs() as i64,
                tv_nsec: i64::from(remaining.subsec_nanos()),
            };
            let ready = {
                let mut descriptors = [PollFd::from_borrowed_fd(
                    guard.connection_fd(),
                    PollFlags::IN | PollFlags::ERR,
                )];
                match poll(&mut descriptors, Some(&timeout)) {
                    Ok(ready) => ready,
                    Err(rustix::io::Errno::INTR) if Instant::now() < deadline => continue,
                    Err(rustix::io::Errno::INTR) => return Ok(0),
                    Err(error) => {
                        return Err(format!("cannot poll Wayland display: {error}"));
                    }
                }
            };
            if ready == 0 {
                return Ok(0);
            }
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    return Ok(0);
                }
                Err(error) => return Err(format!("cannot read Wayland events: {error}")),
            }
            return self
                .queue
                .dispatch_pending(&mut self.state)
                .map_err(|error| format!("cannot dispatch Wayland events: {error}"));
        }
    }

    fn dispatch_queued_inputs(
        &mut self,
        controls: &mut controls::Controls,
    ) -> Result<polling::InputDispatch, String> {
        let control_flags = {
            let mut descriptors = Vec::new();
            controls.append_poll_descriptors(&mut descriptors);
            let _ = polling::wait(&mut descriptors, 0)?;
            descriptors.iter().map(PollFd::revents).collect::<Vec<_>>()
        };
        controls.read_ready(&control_flags);
        Ok(self.input_dispatch(controls, true, false))
    }

    fn dispatch_inputs(
        &mut self,
        controls: &mut controls::Controls,
        timeout_ms: u32,
    ) -> Result<polling::InputDispatch, String> {
        let dispatched = self
            .queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| format!("cannot dispatch Wayland events: {error}"))?;
        if dispatched > 0 || self.state.shutdown || !self.state.touches.is_empty() {
            return self.dispatch_queued_inputs(controls);
        }
        self.flush("cannot flush Wayland display")?;

        let effective_timeout = if controls.report_count() > 0 {
            0
        } else {
            timeout_ms
        };
        let deadline = Instant::now() + Duration::from_millis(u64::from(effective_timeout));
        loop {
            let Some(guard) = self.queue.prepare_read() else {
                let dispatched = self
                    .queue
                    .dispatch_pending(&mut self.state)
                    .map_err(|error| format!("cannot dispatch Wayland events: {error}"))?;
                if dispatched > 0 || self.state.shutdown || !self.state.touches.is_empty() {
                    return self.dispatch_queued_inputs(controls);
                }
                if Instant::now() >= deadline {
                    return Ok(self.input_dispatch(controls, false, false));
                }
                continue;
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            let (touch_flags, control_flags) = {
                let mut descriptors = vec![PollFd::from_borrowed_fd(
                    guard.connection_fd(),
                    PollFlags::IN | PollFlags::ERR,
                )];
                controls.append_poll_descriptors(&mut descriptors);
                let _ = polling::wait_for(&mut descriptors, remaining)?;
                let ready = descriptors.iter().map(PollFd::revents).collect::<Vec<_>>();
                (ready[0], ready[1..].to_vec())
            };

            controls.read_ready(&control_flags);
            let touch_ready = touch_flags
                .intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL);
            if !touch_ready {
                drop(guard);
                return Ok(self.input_dispatch(controls, controls.report_count() > 0, false));
            }
            if !touch_flags.contains(PollFlags::IN) {
                drop(guard);
                eprintln!("retrodeck: Wayland display disconnected");
                return Ok(self.input_dispatch(controls, true, true));
            }

            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    return Ok(self.input_dispatch(controls, true, false));
                }
                Err(error) => {
                    eprintln!("retrodeck: cannot read Wayland events: {error}");
                    return Ok(self.input_dispatch(controls, true, true));
                }
            }
            let touch_lost = match self.queue.dispatch_pending(&mut self.state) {
                Ok(_) => false,
                Err(error) => {
                    eprintln!("retrodeck: cannot dispatch Wayland events: {error}");
                    true
                }
            };
            return Ok(self.input_dispatch(controls, true, touch_lost));
        }
    }

    fn input_dispatch(
        &self,
        controls: &controls::Controls,
        ready: bool,
        touch_lost: bool,
    ) -> polling::InputDispatch {
        polling::InputDispatch {
            ready: ready || controls.report_count() > 0 || !self.state.touches.is_empty(),
            control_count: controls.report_count(),
            touch_count: self.state.touches.len(),
            touch_lost,
            rescan: controls.rescan_requested(),
            shutdown: self.state.shutdown,
        }
    }

    fn flush(&self, context: &str) -> Result<(), String> {
        loop {
            match self.queue.flush() {
                Ok(()) => return Ok(()),
                Err(WaylandError::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    return Ok(());
                }
                Err(WaylandError::Io(error)) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(format!("{context}: {error}")),
            }
        }
    }

    fn ensure_slots(&mut self) -> Result<(), String> {
        let width = self.state.width;
        let height = self.state.height;
        let size = frame_size(width, height)?;
        if self
            .state
            .slots
            .first()
            .is_some_and(|slot| slot.mapping.size == size)
        {
            return Ok(());
        }
        if self.state.slots.iter().any(|slot| slot.busy) {
            return Err("Wayland buffer size changed while buffers are in use".to_owned());
        }
        for slot in self.state.slots.drain(..) {
            slot.buffer.destroy();
        }

        let shm = self
            .state
            .shm
            .clone()
            .expect("checked while opening widget");
        let qh = self.queue.handle();
        let mut slots: Vec<BufferSlot> = Vec::with_capacity(BUFFER_COUNT);
        for index in 0..BUFFER_COUNT {
            let (mapping, fd) = match Mapping::new(size) {
                Ok(result) => result,
                Err(error) => {
                    for slot in slots {
                        slot.buffer.destroy();
                    }
                    return Err(error);
                }
            };
            let pool = shm.create_pool(fd.as_fd(), size as i32, &qh, ());
            let buffer = pool.create_buffer(
                0,
                width as i32,
                height as i32,
                (width * 4) as i32,
                wl_shm::Format::Xrgb8888,
                &qh,
                index,
            );
            pool.destroy();
            slots.push(BufferSlot {
                mapping,
                buffer,
                busy: false,
            });
        }
        self.state.slots = slots;
        Ok(())
    }

    fn present_solid(&mut self, color: u32) -> Result<(), String> {
        self.present_frame(|pixels| pixels.fill(0xff00_0000 | (color & 0x00ff_ffff)))
    }

    fn present_rgba(&mut self, rgba: &[u8]) -> Result<(), String> {
        if self.state.width != canvas::WIDTH
            || self.state.height != canvas::HEIGHT
            || rgba.len() != canvas::WIDTH as usize * canvas::HEIGHT as usize * 4
        {
            return Err("Wayland surface does not match the native canvas".to_owned());
        }
        self.present_frame(|pixels| copy_rgba_to_xrgb(rgba, pixels))
    }

    fn present_rgb565(&mut self, rgb565: &[u16]) -> Result<(), String> {
        if self.state.width != canvas::WIDTH
            || self.state.height != canvas::HEIGHT
            || rgb565.len() != canvas::WIDTH as usize * canvas::HEIGHT as usize
        {
            return Err("Wayland surface does not match the RGB565 frame".to_owned());
        }
        self.present_frame(|pixels| copy_rgb565_to_xrgb(rgb565, pixels))
    }

    fn present_frame(&mut self, draw: impl FnOnce(&mut [u32])) -> Result<(), String> {
        if !self.state.configured {
            return Err("Wayland surface is not configured".to_owned());
        }
        self.ensure_slots()?;
        let _ = self.dispatch(0)?;
        let Some(index) = self.state.slots.iter().position(|slot| !slot.busy) else {
            return Ok(());
        };
        let surface = self
            .state
            .surface
            .clone()
            .expect("created while opening widget");
        let slot = &mut self.state.slots[index];
        draw(slot.mapping.pixels());
        slot.busy = true;
        surface.attach(Some(&slot.buffer), 0, 0);
        surface.damage(0, 0, self.state.width as i32, self.state.height as i32);
        surface.commit();
        self.flush("cannot flush Wayland frame")
    }
}

thread_local! {
    static WIDGET: RefCell<Option<Widget>> = const { RefCell::new(None) };
}

pub fn open_widget() -> Result<(), String> {
    close();
    let widget = Widget::open()?;
    WIDGET.with(|current| *current.borrow_mut() = Some(widget));
    Ok(())
}

pub fn close() {
    WIDGET.with(|current| {
        current.borrow_mut().take();
    });
}

pub fn dispatch(timeout_ms: u32) -> Result<usize, String> {
    with_widget(|widget| widget.dispatch(timeout_ms))
}

pub(crate) fn dispatch_inputs(timeout_ms: u32) -> Result<polling::InputDispatch, String> {
    controls::with_controls(|controls| {
        with_widget(|widget| widget.dispatch_inputs(controls, timeout_ms))
    })
}

pub fn present_solid(color: u32) -> Result<(), String> {
    with_widget(|widget| widget.present_solid(color))
}

pub fn present_rgba(rgba: &[u8]) -> Result<(), String> {
    with_widget(|widget| widget.present_rgba(rgba))
}

pub fn present_rgb565(rgb565: &[u16]) -> Result<(), String> {
    with_widget(|widget| widget.present_rgb565(rgb565))
}

pub fn next_touch() -> Option<TouchReport> {
    WIDGET.with(|current| {
        current
            .borrow_mut()
            .as_mut()
            .and_then(|widget| widget.state.touches.pop_front())
    })
}

pub fn size() -> Option<(u32, u32)> {
    WIDGET.with(|current| {
        current
            .borrow()
            .as_ref()
            .map(|widget| (widget.state.width, widget.state.height))
    })
}

pub fn visible() -> bool {
    WIDGET.with(|current| {
        current
            .borrow()
            .as_ref()
            .is_some_and(|widget| widget.state.visible)
    })
}

pub fn shutdown_requested() -> bool {
    WIDGET.with(|current| {
        current
            .borrow()
            .as_ref()
            .is_some_and(|widget| widget.state.shutdown)
    })
}

fn with_widget<T>(function: impl FnOnce(&mut Widget) -> Result<T, String>) -> Result<T, String> {
    WIDGET.with(|current| {
        let mut current = current.borrow_mut();
        let widget = current
            .as_mut()
            .ok_or_else(|| "Wayland widget is not open".to_owned())?;
        function(widget)
    })
}

fn duration_ms(duration: Duration) -> u32 {
    duration.as_millis().clamp(1, u128::from(u32::MAX)) as u32
}

fn frame_size(width: u32, height: u32) -> Result<usize, String> {
    if width == 0 || height == 0 || width > i32::MAX as u32 / 4 || height > i32::MAX as u32 {
        return Err("Wayland buffer dimensions are invalid".to_owned());
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|size| *size <= i32::MAX as usize)
        .ok_or_else(|| "Wayland buffer dimensions are invalid".to_owned())
}

fn copy_rgba_to_xrgb(rgba: &[u8], pixels: &mut [u32]) {
    debug_assert_eq!(rgba.len(), pixels.len() * 4);
    for (pixel, color) in pixels.iter_mut().zip(rgba.chunks_exact(4)) {
        *pixel = 0xff00_0000
            | (u32::from(color[0]) << 16)
            | (u32::from(color[1]) << 8)
            | u32::from(color[2]);
    }
}

fn copy_rgb565_to_xrgb(rgb565: &[u16], pixels: &mut [u32]) {
    debug_assert_eq!(rgb565.len(), pixels.len());
    for (destination, source) in pixels.iter_mut().zip(rgb565) {
        let red = u32::from((source >> 11) & 0x1f);
        let green = u32::from((source >> 5) & 0x3f);
        let blue = u32::from(source & 0x1f);
        *destination =
            0xff00_0000 | ((red * 255 / 31) << 16) | ((green * 255 / 63) << 8) | (blue * 255 / 31);
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" if state.compositor.is_none() => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "wl_shm" if state.shm.is_none() => {
                    state.shm = Some(registry.bind(name, 1, qh, ()));
                }
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(7), qh, ()));
                }
                "deck_widget_manager_v1" if state.manager.is_none() => {
                    state.manager = Some(registry.bind(name, 1, qh, ()));
                }
                _ => {}
            }
        }
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore deck_widget_manager_v1::DeckWidgetManagerV1);

impl Dispatch<wl_buffer::WlBuffer, usize> for State {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            if let Some(slot) = state.slots.get_mut(*index) {
                slot.busy = false;
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            let have_touch = capabilities.contains(wl_seat::Capability::Touch);
            if have_touch && state.touch.is_none() {
                state.touch = Some(seat.get_touch(qh, ()));
            } else if !have_touch {
                if let Some(touch) = state.touch.take() {
                    if touch.version() >= 3 {
                        touch.release();
                    } else if let Some(backend) = touch.backend().upgrade() {
                        let _ = backend.destroy_object(&touch.id());
                    }
                }
                state.touch_down = false;
            }
        }
    }
}

impl Dispatch<wl_touch::WlTouch, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_touch::WlTouch,
        event: wl_touch::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_touch::Event::Down { surface, x, y, .. } => {
                if state.surface.as_ref() == Some(&surface) {
                    state.touch_x = x as i32;
                    state.touch_y = y as i32;
                    state.touch_down = true;
                    state.push_touch(true, false);
                }
            }
            wl_touch::Event::Up { .. } if state.touch_down => {
                state.touch_down = false;
                state.push_touch(false, true);
            }
            wl_touch::Event::Motion { x, y, .. } if state.touch_down => {
                state.touch_x = x as i32;
                state.touch_y = y as i32;
                state.push_touch(false, false);
            }
            wl_touch::Event::Cancel if state.touch_down => {
                state.touch_down = false;
                state.touches.push_back(TouchReport {
                    x: -1,
                    y: -1,
                    down: false,
                    pressed: false,
                    released: true,
                });
            }
            _ => {}
        }
    }
}

impl Dispatch<deck_widget_surface_v1::DeckWidgetSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &deck_widget_surface_v1::DeckWidgetSurfaceV1,
        event: deck_widget_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            deck_widget_surface_v1::Event::Configure { width, height, .. } => {
                state.width = width;
                state.height = height;
            }
            deck_widget_surface_v1::Event::ConfigureDone => state.configured = true,
            deck_widget_surface_v1::Event::Lifecycle { state: lifecycle } => {
                state.visible = lifecycle != 0;
            }
            deck_widget_surface_v1::Event::Shutdown => state.shutdown = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_frame_geometry() {
        assert_eq!(frame_size(1280, 480).unwrap(), 2_457_600);
        assert!(frame_size(0, 480).is_err());
        assert!(frame_size(i32::MAX as u32, 2).is_err());
        let rgba = [0xfe, 0x6c, 0x27, 0xff, 0xec, 0xb6, 0xe7, 0xff];
        let mut pixels = [0; 2];
        copy_rgba_to_xrgb(&rgba, &mut pixels);
        assert_eq!(pixels, [0xfffe_6c27, 0xffec_b6e7]);
        copy_rgb565_to_xrgb(&[0xfb64, 0xffff], &mut pixels);
        assert_eq!(pixels, [0xffff_6d20, 0xffff_ffff]);
    }

    #[test]
    fn touch_reports_match_dashboard_clamping() {
        let mut state = State {
            width: 1280,
            height: 480,
            touch_x: 1300,
            touch_y: -3,
            touch_down: true,
            ..State::default()
        };
        state.push_touch(true, false);
        assert_eq!(
            state.touches.pop_front(),
            Some(TouchReport {
                x: 1279,
                y: 0,
                down: true,
                pressed: true,
                released: false,
            })
        );
    }
}
