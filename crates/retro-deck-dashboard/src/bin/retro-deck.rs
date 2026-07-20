//! Native Retro Deck scene for the BMC Wayland compositor.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context as _, Result};
use bmc_gpu_render_lock::GpuRenderLock;
use bmc_render::gpu::FemtoVgRenderer;
use bmc_render::interaction::TouchEvent;
use bmc_render::renderer::{FrameClear, Renderer as _};
use bmc_render::{TreeUi, tree::TreeResult};
use bmc_widget::ActionPayload;
use bmc_widget::egl::{
    Depth, DoubleBufferState, EglContext, SharedRenderScratch, SlotReleaseState,
};
use bmc_widget::surface::{
    DeckWidgetSurfaceClient, LifecycleState, WidgetEvent, WidgetSurface as _,
};
use glow::HasContext as _;
use retro_deck_dashboard::{
    BmcScreen, BmcUiAction, Brightness, DashboardModel, Keymap, MenuCue, VolumeState,
    bmc_action_for_touch, build_bmc_tree, load_native_catalog,
};

const MANIFEST_ENV: &str = "RETRO_DECK_MANIFEST";
const DEFAULT_MANIFEST_PATH: &str = "/mnt/data/nes-deck/menu/games.tsv";

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
    let catalog = load_native_catalog(configured_path(MANIFEST_ENV, DEFAULT_MANIFEST_PATH))
        .context("load Retro Deck catalog")?;
    let model = DashboardModel::new(
        catalog,
        VolumeState::DEFAULT,
        Brightness::DEFAULT,
        Keymap::default(),
    );
    let (surface, initial) =
        DeckWidgetSurfaceClient::connect().context("connect Retro Deck scene")?;
    let runtime = NativeRuntime::new(surface, model, (initial.width, initial.height));
    runtime.run()
}

fn configured_path(variable: &str, default: &str) -> PathBuf {
    env::var_os(variable).map_or_else(|| PathBuf::from(default), PathBuf::from)
}

struct NativeRuntime {
    graphics: Option<Graphics>,
    surface: DeckWidgetSurfaceClient,
    tree_ui: TreeUi,
    model: DashboardModel,
    screen: BmcScreen,
    size: (u32, u32),
    lifecycle: Option<LifecycleState>,
    pending_render: bool,
    active_touch: Option<i32>,
    started_at: Instant,
    last_frame_at: Option<Instant>,
}

impl NativeRuntime {
    fn new(surface: DeckWidgetSurfaceClient, model: DashboardModel, size: (u32, u32)) -> Self {
        Self {
            graphics: None,
            surface,
            tree_ui: TreeUi::new(),
            model,
            screen: BmcScreen::Categories,
            size,
            lifecycle: None,
            pending_render: true,
            active_touch: None,
            started_at: Instant::now(),
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
            self.release_dormant_buffers();
            self.render_if_ready()?;

            let timeout_ms = if self.can_render_now() { 0 } else { -1 };
            self.surface
                .poll_dispatch(timeout_ms)
                .context("dispatch Retro Deck Wayland events")?;
        }
        Ok(())
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
                WidgetEvent::Shutdown => self.surface.request_shutdown(),
                WidgetEvent::TransitionIncoming => {
                    if has_render_target(self.lifecycle) {
                        self.pending_render = true;
                    }
                }
                WidgetEvent::Setting(_) | WidgetEvent::ParamUpdate(_) => {}
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
        let tree = build_bmc_tree(&self.model, self.screen, self.size);
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
            let Some(action) = bmc_action_for_touch(self.screen, &key) else {
                continue;
            };
            match action {
                BmcUiAction::OpenCarousel => {
                    self.screen = BmcScreen::Carousel;
                    self.pending_render = true;
                    self.play_menu_cue(MenuCue::Confirm);
                }
                BmcUiAction::CloseCarousel => {
                    self.screen = BmcScreen::Categories;
                    self.pending_render = true;
                    self.play_menu_cue(MenuCue::Back);
                }
                BmcUiAction::Model(action) => {
                    let elapsed_ms =
                        u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                    let transition = self.model.apply_at(action, elapsed_ms);
                    self.pending_render |= transition.redraw;
                    if let Some(cue) = transition.cue {
                        self.play_menu_cue(cue);
                    }
                    if let Some(intent) = transition.intent {
                        tracing::info!(?intent, "native launch integration remains pending");
                    }
                }
            }
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

struct Graphics {
    egl: EglContext,
    scratch: Option<SharedRenderScratch>,
    renderer: Option<FemtoVgRenderer>,
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
        let renderer = match renderer {
            Ok(renderer) => renderer,
            Err(error) => {
                scratch.destroy(&egl);
                return Err(error).context("create BMC renderer");
            }
        };
        Ok(Self {
            egl,
            scratch: Some(scratch),
            renderer: Some(renderer),
            buffers: DoubleBufferState::new(size.0, size.1, Depth::Disabled),
            release: SlotReleaseState::new(),
            gpu_lock,
        })
    }

    fn current_buffer_available(&self) -> bool {
        self.release.is_available(self.buffers.current_slot())
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
        MenuCue::Confirm | MenuCue::Volume => "Confirmation",
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
        assert_eq!(menu_cue_sound(MenuCue::Volume), "Confirmation");
    }
}
