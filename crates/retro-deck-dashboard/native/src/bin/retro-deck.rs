//! Native Retro Deck scene for the BMC Wayland compositor.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use bmc_gpu_render_lock::GpuRenderLock;
use bmc_render::gpu::FemtoVgRenderer;
use bmc_render::interaction::TouchEvent;
use bmc_render::renderer::{FrameClear, Renderer as _};
use bmc_render::{BitmapId, TreeUi, tree::TreeResult};
use bmc_widget::ActionPayload;
use bmc_widget::egl::{
    Depth, DoubleBufferState, EglContext, SharedRenderScratch, SlotReleaseState,
};
use bmc_widget::surface::{
    DeckWidgetSurfaceClient, KeyboardKeyState, LifecycleState, WidgetEvent, WidgetSurface as _,
};
use glow::HasContext as _;
use retro_deck_dashboard::{
    ApplicationRequest, BMC_APPLICATION_ID, BmcNavigation, BmcUiAction, DashboardModel,
    GamepadInput, Intent, Keymap, LaunchTarget, MenuCue, NATIVE_COVER_SIZE, VolumeState,
    bmc_action_for_navigation, bmc_action_for_touch, build_bmc_tree, dashboard_startup_from_policy,
    load_native_catalog, load_native_catalog_with_uploads, load_native_cover, load_native_palette,
    visible_catalog_indices,
};
use retro_deck_policy::{
    PolicyClient, PolicyEvent, PolicyEventPoll, PolicyResponse, PolicySubmit, Value, WorkerCommand,
    WorkerConfig,
};

const MANIFEST_ENV: &str = "RETRO_DECK_MANIFEST";
const DEFAULT_MANIFEST_PATH: &str = "/mnt/data/nes-deck/menu/games.tsv";
const UPLOAD_MANIFEST_PATH: &str = "/mnt/data/nes-deck/uploads/games.tsv";
const DEFAULT_PALETTE_PATH: &str = "/mnt/data/nes-deck/menu/palette.tsv";
const PALETTE_OVERRIDE_PATH: &str = "/mnt/data/nes-deck/state/dashboard-palette.sexp";
const COVER_DIRECTORY: &str = "/mnt/data/nes-deck/covers";
const ECL_PROGRAM: &str = "/mnt/data/nes-deck/ecl/bin/ecl.bin";
const ECL_DIRECTORY: &str = "/mnt/data/nes-deck/ecl/lib/ecl/";
const LISP_DIRECTORY: &str = "/mnt/data/nes-deck/lisp";
const LISP_WORKER: &str = "/mnt/data/nes-deck/lisp/run-worker.lisp";
const LISP_SITE_DIRECTORY: &str = "/mnt/data/nes-deck/lisp/site.d";
const DASHBOARD_POLICY_HOOK: &str = "dashboard/startup";
const POLICY_POLL_INTERVAL: Duration = Duration::from_millis(25);
const SETTINGS_COG_FILE: &str = "gear-knekko-09.png";
const COVER_BITMAP_TAG: &str = "retro-deck-cover";
const KEY_ESCAPE: u32 = 1;
const KEY_ENTER: u32 = 28;
const KEY_SPACE: u32 = 57;
const KEY_KEYPAD_ENTER: u32 = 96;
const KEY_UP: u32 = 103;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;
const KEY_DOWN: u32 = 108;

fn main() -> ExitCode {
    bmc_log::init_console();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error = ?error, "Retro Deck stopped");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let configured_manifest = configured_path(MANIFEST_ENV, DEFAULT_MANIFEST_PATH);
    let catalog = if env::var_os(MANIFEST_ENV).is_some() {
        load_native_catalog(&configured_manifest)
    } else {
        load_native_catalog_with_uploads(&configured_manifest, Path::new(UPLOAD_MANIFEST_PATH))
    }
    .context("load Retro Deck catalog")?;
    let palette = load_native_palette(
        Path::new(DEFAULT_PALETTE_PATH),
        Path::new(PALETTE_OVERRIDE_PATH),
    );
    let model = DashboardModel::new(catalog, VolumeState::DEFAULT, Keymap::default());
    let (surface, initial) =
        DeckWidgetSurfaceClient::connect().context("connect Retro Deck scene")?;
    let policy = spawn_dashboard_policy();
    let runtime = NativeRuntime::new(
        surface,
        model,
        palette,
        (initial.width, initial.height),
        policy,
    );
    runtime.run()
}

fn configured_path(variable: &str, default: &str) -> PathBuf {
    env::var_os(variable).map_or_else(|| PathBuf::from(default), PathBuf::from)
}

fn widget_asset_path(file: &str) -> PathBuf {
    let executable = env::current_exe().ok();
    executable
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map_or_else(
            || Path::new("assets").join(file),
            |widget_directory| widget_directory.join("assets").join(file),
        )
}

struct NativeRuntime {
    graphics: Option<Graphics>,
    surface: DeckWidgetSurfaceClient,
    tree_ui: TreeUi,
    model: DashboardModel,
    palette: retro_deck_config::Palette,
    size: (u32, u32),
    lifecycle: Option<LifecycleState>,
    pending_render: bool,
    active_touch: Option<i32>,
    gamepad: GamepadInput,
    policy: Option<PolicyClient>,
    last_frame_at: Option<Instant>,
}

impl NativeRuntime {
    fn new(
        surface: DeckWidgetSurfaceClient,
        model: DashboardModel,
        palette: retro_deck_config::Palette,
        size: (u32, u32),
        policy: Option<PolicyClient>,
    ) -> Self {
        Self {
            graphics: None,
            surface,
            tree_ui: TreeUi::new(),
            model,
            palette,
            size,
            lifecycle: None,
            pending_render: true,
            active_touch: None,
            gamepad: GamepadInput::default(),
            policy,
            last_frame_at: None,
        }
    }

    fn run(mut self) -> Result<()> {
        let result = self.event_loop();
        self.surface.invalidate_cached_buffers();
        if let Err(error) = self.surface.flush() {
            tracing::warn!(?error, "failed to flush buffer cleanup");
        }
        drop(self.graphics.take());
        result
    }

    fn event_loop(&mut self) -> Result<()> {
        while self.surface.running() {
            self.drain_surface_events();
            self.drain_policy_events();
            self.release_dormant_buffers();
            self.render_if_ready()?;

            let timeout_ms = if self.can_render_now() {
                0
            } else if self.policy.is_some() {
                i32::try_from(POLICY_POLL_INTERVAL.as_millis()).unwrap_or(25)
            } else {
                -1
            };
            self.surface
                .poll_dispatch(timeout_ms)
                .context("dispatch Retro Deck Wayland events")?;
        }
        Ok(())
    }

    fn drain_policy_events(&mut self) {
        let event = match self.policy.as_mut() {
            Some(policy) => policy.try_event(),
            None => return,
        };
        match event {
            PolicyEventPoll::Empty => {}
            PolicyEventPoll::Disconnected => {
                tracing::warn!("Common Lisp dashboard policy disconnected");
                self.policy.take();
            }
            PolicyEventPoll::Event(PolicyEvent::Response(response)) => {
                self.apply_policy_response(response);
                self.policy.take();
            }
            PolicyEventPoll::Event(PolicyEvent::Unavailable(failure)) => {
                tracing::warn!(?failure, "Common Lisp dashboard policy unavailable");
                self.policy.take();
            }
        }
    }

    fn apply_policy_response(&mut self, response: PolicyResponse) {
        let PolicyResponse::Ok { value, .. } = response else {
            tracing::warn!(?response, "Common Lisp dashboard policy rejected startup");
            return;
        };
        let (applications, gamepad) = match dashboard_startup_from_policy(&value) {
            Ok(startup) => startup,
            Err(error) => {
                tracing::warn!(?error, "invalid Common Lisp dashboard startup policy");
                return;
            }
        };
        let catalog = match retro_deck_dashboard::DashboardCatalog::from_entries(
            self.model
                .catalog()
                .entries()
                .iter()
                .cloned()
                .chain(applications),
        ) {
            Ok(catalog) => catalog,
            Err(error) => {
                tracing::warn!(?error, "Common Lisp dashboard applications conflict");
                return;
            }
        };
        self.model = DashboardModel::new(catalog, self.model.volume(), self.model.keymap());
        self.gamepad.set_profile(gamepad);
        self.pending_render = true;
        tracing::info!("loaded dashboard startup policy from Common Lisp");
    }

    fn drain_surface_events(&mut self) {
        if self.surface.take_render_requested() {
            self.pending_render = true;
        }
        for slot in self.surface.drain_released_slots() {
            if let Some(graphics) = self.graphics.as_mut() {
                graphics.release.mark_released(slot);
            }
        }
        for event in self.surface.drain_events() {
            match event {
                WidgetEvent::Lifecycle(state) => self.apply_lifecycle(state),
                WidgetEvent::TouchDown { id, x, y } => {
                    if self.active_touch.is_none() {
                        self.active_touch = Some(id);
                        self.tree_ui.push_touch(touch_down(x, y));
                        self.pending_render = true;
                    }
                }
                WidgetEvent::TouchMotion { id, x, y } => {
                    if self.active_touch == Some(id) {
                        self.tree_ui.push_touch(touch_move(x, y));
                        self.pending_render = true;
                    }
                }
                WidgetEvent::TouchUp { id } => {
                    if self.active_touch == Some(id) {
                        self.active_touch = None;
                        self.tree_ui.push_touch(TouchEvent::Up);
                        self.pending_render = true;
                    }
                }
                WidgetEvent::TouchCancel => {
                    self.cancel_touch();
                    self.pending_render = true;
                }
                WidgetEvent::Key {
                    key,
                    state: KeyboardKeyState::Pressed,
                } => {
                    if let Some(navigation) = navigation_for_linux_key(key) {
                        self.apply_navigation(navigation);
                    }
                }
                WidgetEvent::Key {
                    state: KeyboardKeyState::Released,
                    ..
                }
                | WidgetEvent::Setting(_)
                | WidgetEvent::ParamUpdate(_) => {}
                WidgetEvent::Gamepad(event) => {
                    for navigation in self.gamepad.handle(&event) {
                        self.apply_navigation(navigation);
                    }
                }
                WidgetEvent::Shutdown => self.surface.request_shutdown(),
                WidgetEvent::TransitionIncoming => {
                    if has_render_target(self.lifecycle) {
                        self.pending_render = true;
                    }
                }
            }
        }
    }

    fn apply_lifecycle(&mut self, state: LifecycleState) {
        if self.lifecycle == Some(state) {
            return;
        }
        let previous = self.lifecycle;
        self.lifecycle = Some(state);
        if has_render_target(self.lifecycle) {
            self.pending_render = true;
        } else {
            self.pending_render = false;
            self.cancel_touch();
            self.gamepad.reset();
        }
        tracing::info!(?previous, ?state, "Retro Deck lifecycle changed");
    }

    fn cancel_touch(&mut self) {
        self.active_touch = None;
        self.tree_ui.cancel_touch();
    }

    fn can_render_now(&self) -> bool {
        self.pending_render
            && has_render_target(self.lifecycle)
            && self
                .graphics
                .as_ref()
                .is_none_or(Graphics::current_buffer_available)
    }

    fn render_if_ready(&mut self) -> Result<()> {
        if !self.pending_render || !has_render_target(self.lifecycle) {
            return Ok(());
        }
        if self.graphics.is_none() {
            self.graphics = Some(Graphics::new(self.size)?);
        }
        let Some(graphics) = self.graphics.as_mut() else {
            anyhow::bail!("graphics initialization returned no graphics state");
        };
        if !graphics.current_buffer_available() {
            return Ok(());
        }

        let now = Instant::now();
        let delta_ms = self.last_frame_at.replace(now).map_or(0, |previous| {
            u32::try_from(now.duration_since(previous).as_millis()).unwrap_or(u32::MAX)
        });
        let cover_requests = visible_catalog_indices(&self.model)
            .into_iter()
            .filter_map(|index| {
                self.model
                    .catalog()
                    .entry(index)
                    .map(|entry| (index, entry.identifier().to_owned()))
            })
            .collect::<Vec<_>>();
        let covers = graphics.select_covers(&cover_requests)?;
        let tree = build_bmc_tree(
            &self.model,
            self.size,
            &self.palette,
            &covers,
            graphics.settings_cog,
        );
        let result = graphics.render(
            &mut self.surface,
            &mut self.tree_ui,
            &tree,
            self.size,
            delta_ms,
        )?;
        self.pending_render = false;
        self.apply_tree_result(result);
        Ok(())
    }

    fn apply_tree_result(&mut self, result: TreeResult) {
        for key in result.clicks.into_keys() {
            let Some(action) = bmc_action_for_touch(&key) else {
                continue;
            };
            self.apply_ui_action(action);
        }
    }

    fn apply_navigation(&mut self, navigation: BmcNavigation) {
        if let Some(action) = bmc_action_for_navigation(navigation) {
            self.apply_ui_action(action);
        }
    }

    fn apply_ui_action(&mut self, action: BmcUiAction) {
        match action {
            BmcUiAction::OpenSystemSettings => {
                self.open_system_settings();
                self.play_menu_cue(MenuCue::Confirm);
            }
            BmcUiAction::Launch(index) => {
                self.play_menu_cue(MenuCue::Confirm);
                self.request_intent(Intent::Launch(index));
            }
            BmcUiAction::Model(action) => {
                let transition = self.model.apply(action);
                self.pending_render |= transition.redraw;
                if let Some(cue) = transition.cue {
                    self.play_menu_cue(cue);
                }
                if let Some(intent) = transition.intent {
                    self.request_intent(intent);
                }
            }
        }
    }

    fn request_intent(&self, intent: Intent) {
        let Intent::Launch(index) = intent;
        let Some(entry) = self.model.catalog().entry(index) else {
            tracing::warn!(index, "rejected missing native catalog launch");
            return;
        };
        let target = match LaunchTarget::from_entry(entry) {
            Ok(target) => target,
            Err(error) => {
                tracing::warn!(index, ?error, "rejected unknown native catalog launch");
                return;
            }
        };
        if target == LaunchTarget::Reboot {
            self.open_system_settings();
            return;
        }
        let request =
            match ApplicationRequest::from_target(target, self.model.volume(), self.model.keymap())
            {
                Ok(request) => request,
                Err(error) => {
                    tracing::warn!(?error, "native launch requires a different BMC capability");
                    return;
                }
            };
        let input = match serde_json::to_string(&request) {
            Ok(input) => input,
            Err(error) => {
                tracing::warn!(?error, "failed to encode native application request");
                return;
            }
        };
        let action = ActionPayload::LaunchApplication {
            application_id: BMC_APPLICATION_ID.to_owned(),
            input,
        };
        if let Err(error) = self.surface.request_action(&action) {
            tracing::warn!(?error, "BMC application launch request failed");
        }
    }

    fn play_menu_cue(&self, cue: MenuCue) {
        if self.model.volume().is_muted() {
            return;
        }
        let action = ActionPayload::PlaySound {
            sound: menu_cue_sound(cue).to_owned(),
        };
        if let Err(error) = self.surface.request_action(&action) {
            tracing::warn!(?error, ?cue, "BMC menu sound request failed");
        }
    }

    fn open_system_settings(&self) {
        if let Err(error) = self
            .surface
            .request_action(&ActionPayload::OpenSystemSettings {})
        {
            tracing::warn!(?error, "BMC system settings request failed");
        }
    }

    fn release_dormant_buffers(&mut self) {
        if self.lifecycle != Some(LifecycleState::Dormant) {
            return;
        }
        let Some(graphics) = self.graphics.as_mut() else {
            return;
        };
        let released = graphics.destroy_released_buffers();
        if !released.is_empty() {
            self.surface.invalidate_cached_buffer_slots(&released);
        }
    }
}

fn spawn_dashboard_policy() -> Option<PolicyClient> {
    let command = WorkerCommand::new(ECL_PROGRAM)
        .arg("--norc")
        .arg("--shell")
        .arg(LISP_WORKER)
        .env("ECLDIR", ECL_DIRECTORY)
        .env("RETRO_DECK_LISP_SITE_DIR", LISP_SITE_DIRECTORY)
        .current_dir(LISP_DIRECTORY);
    match PolicyClient::spawn(WorkerConfig::new(command)) {
        Ok(mut policy) => match policy.try_submit(DASHBOARD_POLICY_HOOK, Value::Nil) {
            Ok(PolicySubmit::Queued(_)) => Some(policy),
            Ok(PolicySubmit::Unavailable) => {
                tracing::warn!("Common Lisp dashboard policy rejected startup work");
                None
            }
            Err(error) => {
                tracing::warn!(?error, "cannot encode dashboard startup policy");
                None
            }
        },
        Err(error) => {
            tracing::warn!(?error, "cannot start Common Lisp dashboard supervisor");
            None
        }
    }
}

const fn navigation_for_linux_key(key: u32) -> Option<BmcNavigation> {
    match key {
        KEY_ESCAPE => Some(BmcNavigation::Back),
        KEY_ENTER | KEY_SPACE | KEY_KEYPAD_ENTER => Some(BmcNavigation::Confirm),
        KEY_UP => Some(BmcNavigation::Up),
        KEY_LEFT => Some(BmcNavigation::Left),
        KEY_RIGHT => Some(BmcNavigation::Right),
        KEY_DOWN => Some(BmcNavigation::Down),
        _ => None,
    }
}

struct Graphics {
    egl: EglContext,
    scratch: Option<SharedRenderScratch>,
    renderer: Option<FemtoVgRenderer>,
    settings_cog: Option<BitmapId>,
    cover_signature: Vec<(usize, Box<str>)>,
    cover_bitmaps: Vec<(usize, BitmapId)>,
    buffers: DoubleBufferState,
    release: SlotReleaseState,
    gpu_lock: GpuRenderLock,
}

impl Graphics {
    fn new(size: (u32, u32)) -> Result<Self> {
        let gpu_lock = GpuRenderLock::from_env().context("open BMC GPU render lock")?;
        let egl = EglContext::new().context("initialize BMC EGL")?;
        let scratch = SharedRenderScratch::new(&egl, size.0, size.1)
            .context("allocate BMC render scratch")?;
        // SAFETY: Retro Deck renders on this one thread while `egl` remains
        // current, and `scratch` retains the renderer's target FBO.
        let renderer = unsafe {
            FemtoVgRenderer::new(
                EglContext::get_proc_address,
                size.0,
                size.1,
                scratch.staging_fbo_id(),
                0,
            )
        };
        let mut renderer = match renderer {
            Ok(renderer) => renderer,
            Err(error) => {
                scratch.destroy(&egl);
                return Err(error).context("create BMC renderer");
            }
        };
        let settings_cog_path = widget_asset_path(SETTINGS_COG_FILE);
        let settings_cog = match std::fs::read(&settings_cog_path) {
            Ok(png) => renderer.register_bitmap_nearest("retro-deck:settings-cog", &png),
            Err(error) => {
                tracing::warn!(?error, path = %settings_cog_path.display(), "cannot load settings cog");
                None
            }
        };
        Ok(Self {
            egl,
            scratch: Some(scratch),
            renderer: Some(renderer),
            settings_cog,
            cover_signature: Vec::new(),
            cover_bitmaps: Vec::new(),
            buffers: DoubleBufferState::new(size.0, size.1, Depth::Disabled),
            release: SlotReleaseState::new(),
            gpu_lock,
        })
    }

    fn current_buffer_available(&self) -> bool {
        self.release.is_available(self.buffers.current_slot())
    }

    fn select_covers(&mut self, requests: &[(usize, String)]) -> Result<Vec<(usize, BitmapId)>> {
        let unchanged = self.cover_signature.len() == requests.len()
            && self.cover_signature.iter().zip(requests).all(
                |((stored_index, stored_identifier), (index, identifier))| {
                    stored_index == index && stored_identifier.as_ref() == identifier
                },
            );
        if unchanged {
            return Ok(self.cover_bitmaps.clone());
        }
        let mut decoded = Vec::with_capacity(requests.len());
        for (index, identifier) in requests {
            match load_native_cover(Path::new(COVER_DIRECTORY), identifier) {
                Ok(Some(cover)) => decoded.push((*index, cover)),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(?error, identifier, "cannot load cached dashboard cover");
                }
            }
        }
        let _lock = self.gpu_lock.lock("retro_deck_cover")?;
        let Some(renderer) = self.renderer.as_mut() else {
            anyhow::bail!("renderer was destroyed before cover selection");
        };
        renderer.evict_prefix(COVER_BITMAP_TAG);
        let mut bitmaps = Vec::with_capacity(decoded.len());
        for (index, cover) in decoded {
            let tag = format!("{COVER_BITMAP_TAG}:{index}");
            if let Some(bitmap) = renderer.register_bitmap_rgba(
                &tag,
                cover.rgba(),
                NATIVE_COVER_SIZE,
                NATIVE_COVER_SIZE,
            ) {
                bitmaps.push((index, bitmap));
            }
        }
        self.cover_signature = requests
            .iter()
            .map(|(index, identifier)| (*index, identifier.clone().into_boxed_str()))
            .collect();
        self.cover_bitmaps.clone_from(&bitmaps);
        Ok(bitmaps)
    }

    fn render(
        &mut self,
        surface: &mut DeckWidgetSurfaceClient,
        tree_ui: &mut TreeUi,
        tree: &bmc_render::tree::TreeNode,
        size: (u32, u32),
        delta_ms: u32,
    ) -> Result<TreeResult> {
        let lock = self.gpu_lock.lock("retro_deck")?;
        self.buffers
            .ensure_current(&self.egl)
            .context("allocate Retro Deck export buffer")?;
        let Some(scratch) = self.scratch.as_ref() else {
            anyhow::bail!("render scratch was destroyed before shutdown");
        };
        let Some(renderer) = self.renderer.as_mut() else {
            anyhow::bail!("renderer was destroyed before shutdown");
        };
        let _staging = scratch.begin_frame(&self.egl, size.0, size.1);
        renderer.begin_frame_with_clear(size.0, size.1, 1.0, FrameClear::OpaqueBlack);
        let result = tree_ui
            .render(tree, size, delta_ms, renderer)
            .context("render Retro Deck tree")?;
        renderer.flush();
        let export_fbo = self
            .buffers
            .current_ref()
            .context("export buffer disappeared after allocation")?
            .fbo;
        scratch.blit_to(&self.egl, export_fbo, size.0, size.1);
        wait_for_gpu(&self.egl);
        drop(lock);

        let (dmabuf, slot) = self.buffers.export_and_swap()?;
        surface.submit_buffer(&dmabuf, slot, false)?;
        surface.flush()?;
        self.release.mark_presented(slot);
        Ok(result)
    }

    fn destroy_released_buffers(&mut self) -> Vec<usize> {
        let slots = self
            .release
            .destroyable_slots(self.buffers.allocated_slots())
            .collect::<Vec<_>>();
        for slot in &slots {
            self.buffers.destroy_slot(&self.egl, *slot);
        }
        slots
    }
}

impl Drop for Graphics {
    fn drop(&mut self) {
        drop(self.renderer.take());
        self.buffers.destroy_all(&self.egl);
        if let Some(scratch) = self.scratch.take() {
            scratch.destroy(&self.egl);
        }
    }
}

const fn has_render_target(lifecycle: Option<LifecycleState>) -> bool {
    matches!(
        lifecycle,
        Some(
            LifecycleState::Prepared
                | LifecycleState::Entering
                | LifecycleState::Visible
                | LifecycleState::Leaving
        )
    )
}

const fn menu_cue_sound(cue: MenuCue) -> &'static str {
    match cue {
        MenuCue::Previous | MenuCue::Back => "PriceDown",
        MenuCue::Next => "PriceUp",
        MenuCue::Confirm => "Confirmation",
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "Deck touch coordinates are bounded by the configured viewport"
)]
const fn touch_down(x: f64, y: f64) -> TouchEvent {
    TouchEvent::Down {
        x: x as f32,
        y: y as f32,
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "Deck touch coordinates are bounded by the configured viewport"
)]
const fn touch_move(x: f64, y: f64) -> TouchEvent {
    TouchEvent::Move {
        x: x as f32,
        y: y as f32,
    }
}

fn wait_for_gpu(egl: &EglContext) {
    if let Err(error) = egl.wait_for_egl_fence() {
        tracing::warn!(?error, "EGL fence failed; falling back to glFinish");
        // SAFETY: the EGL context is current on the native widget thread.
        unsafe {
            egl.gl().finish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_and_swipe_states_keep_a_render_target() {
        for state in [
            LifecycleState::Prepared,
            LifecycleState::Entering,
            LifecycleState::Visible,
            LifecycleState::Leaving,
        ] {
            assert!(has_render_target(Some(state)));
        }
        assert!(!has_render_target(Some(LifecycleState::Dormant)));
        assert!(!has_render_target(None));
    }

    #[test]
    fn menu_cues_use_finite_bmc_sounds() {
        assert_eq!(menu_cue_sound(MenuCue::Previous), "PriceDown");
        assert_eq!(menu_cue_sound(MenuCue::Next), "PriceUp");
        assert_eq!(menu_cue_sound(MenuCue::Confirm), "Confirmation");
        assert_eq!(menu_cue_sound(MenuCue::Back), "PriceDown");
    }

    #[test]
    fn linux_navigation_keys_map_without_layout_dependent_text() {
        assert_eq!(navigation_for_linux_key(KEY_UP), Some(BmcNavigation::Up));
        assert_eq!(
            navigation_for_linux_key(KEY_DOWN),
            Some(BmcNavigation::Down)
        );
        assert_eq!(
            navigation_for_linux_key(KEY_LEFT),
            Some(BmcNavigation::Left)
        );
        assert_eq!(
            navigation_for_linux_key(KEY_RIGHT),
            Some(BmcNavigation::Right)
        );
        for key in [KEY_ENTER, KEY_SPACE, KEY_KEYPAD_ENTER] {
            assert_eq!(navigation_for_linux_key(key), Some(BmcNavigation::Confirm));
        }
        assert_eq!(
            navigation_for_linux_key(KEY_ESCAPE),
            Some(BmcNavigation::Back)
        );
        assert_eq!(navigation_for_linux_key(u32::MAX), None);
    }
}
