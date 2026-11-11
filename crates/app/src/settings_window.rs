//! The settings window, drawn with egui on the shared GPU device.

use std::sync::Arc;

use anyhow::{Context, Result};
use freeflake_core::{Intensity, Settings};
use freeflake_render::{Gpu, wgpu};
use glam::Vec2;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::i18n::{S, t};

/// One-off requests from the window besides settings changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Shovel,
    SetAutostart(bool),
}

pub struct SettingsWindow {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
}

impl SettingsWindow {
    pub fn open(event_loop: &ActiveEventLoop, gpu: &Gpu) -> Result<Self> {
        let attrs = Window::default_attributes()
            .with_title(format!("freeflake — {}", t(S::SettingsTitle)))
            .with_inner_size(LogicalSize::new(440.0, 640.0))
            .with_min_inner_size(LogicalSize::new(380.0, 420.0));
        let window = Arc::new(event_loop.create_window(attrs)?);
        let surface = gpu
            .instance
            .create_surface(window.clone())
            .context("creating settings surface")?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            format,
            present_mode: wgpu::PresentMode::AutoVsync,
            ..surface
                .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
                .context("settings surface unsupported")?
        };
        surface.configure(&gpu.device, &config);
        let ctx = egui::Context::default();
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let renderer = egui_wgpu::Renderer::new(&gpu.device, format, Default::default());
        window.focus_window();
        Ok(Self {
            window,
            surface,
            config,
            ctx,
            state,
            renderer,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn focus(&self) {
        self.window.focus_window();
    }

    /// Feeds a window event to egui. Returns false when the window should
    /// close.
    pub fn on_event(&mut self, gpu: &Gpu, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CloseRequested => return false,
            WindowEvent::Resized(size) => {
                self.config.width = size.width.max(1);
                self.config.height = size.height.max(1);
                self.surface.configure(&gpu.device, &self.config);
            }
            _ => {}
        }
        let response = self.state.on_window_event(&self.window, event);
        if response.repaint {
            self.window.request_redraw();
        }
        true
    }

    /// Draws the window, editing `settings` in place.
    pub fn redraw(&mut self, gpu: &Gpu, settings: &mut Settings, autostart: bool) -> Vec<Request> {
        let mut requests = Vec::new();
        let input = self.state.take_egui_input(&self.window);
        let output = self.ctx.run_ui(input, |ui| {
            egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    contents(ui, settings, autostart, &mut requests);
                });
            });
        });
        self.state
            .handle_platform_output(&self.window, output.platform_output);
        let ppp = output.pixels_per_point;
        let jobs = self.ctx.tessellate(output.shapes, ppp);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ppp,
        };
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                self.surface.configure(&gpu.device, &self.config);
                self.window.request_redraw();
                return requests;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let extra =
            self.renderer
                .update_buffers(&gpu.device, &gpu.queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("settings"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &jobs, &screen);
        }
        gpu.queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        gpu.queue.present(frame);
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        let repaint = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|v| v.repaint_delay.is_zero());
        if repaint {
            self.window.request_redraw();
        }
        requests
    }
}

fn contents(ui: &mut egui::Ui, s: &mut Settings, autostart: bool, requests: &mut Vec<Request>) {
    ui.spacing_mut().item_spacing.y = 8.0;
    ui.heading(t(S::Snowfall));
    ui.horizontal(|ui| {
        for (i, label) in Intensity::ALL
            .into_iter()
            .zip([S::Light, S::Steady, S::Blizzard])
        {
            ui.selectable_value(&mut s.intensity, i, t(label));
        }
    });
    ui.checkbox(&mut s.paused, t(S::Pause));
    ui.checkbox(&mut s.snow_over_windows, t(S::SnowOverWindows));

    ui.separator();
    ui.heading(t(S::Wind));
    ui.label(t(S::WindHint));
    ui.horizontal(|ui| {
        wind_pad(ui, &mut s.wind);
        ui.vertical(|ui| {
            if ui.button(t(S::WindCalm)).clicked() {
                s.wind = Vec2::ZERO;
            }
            ui.label(format!(
                "{:+.0}%  {:+.0}%",
                s.wind.x * 100.0,
                s.wind.y * 100.0
            ));
        });
    });

    ui.separator();
    ui.heading(t(S::Accumulate));
    ui.checkbox(&mut s.accumulate, t(S::Accumulate));
    ui.add_enabled_ui(s.accumulate, |ui| {
        ui.add(egui::Slider::new(&mut s.max_window_drift, 0.0..=80.0).text(t(S::MaxWindowPile)));
        ui.add(egui::Slider::new(&mut s.max_ground_drift, 0.0..=200.0).text(t(S::MaxGroundPile)));
        ui.checkbox(&mut s.melt, t(S::Melt));
        ui.checkbox(&mut s.mitten, t(S::Mitten));
        ui.label(t(S::SnowballHint));
        if ui.button(t(S::Shovel)).clicked() {
            requests.push(Request::Shovel);
        }
    });

    ui.separator();
    ui.heading(t(S::Tree));
    ui.checkbox(&mut s.show_tree, t(S::ShowTree));
    ui.label(t(S::TreeHint));

    ui.separator();
    ui.heading(t(S::Performance));
    ui.add(egui::Slider::new(&mut s.fps, 20..=120).text(t(S::Fps)));
    ui.checkbox(&mut s.save_power, t(S::SavePower));
    ui.add_enabled(
        s.save_power,
        egui::Slider::new(&mut s.calm_fps, 10..=60).text(t(S::CalmFps)),
    );
    ui.add(egui::Slider::new(&mut s.low_power_fps, 10..=60).text(t(S::LowPowerFps)));
    let mut on = autostart;
    if ui.checkbox(&mut on, t(S::Autostart)).changed() {
        requests.push(Request::SetAutostart(on));
    }

    ui.separator();
    ui.hyperlink_to(t(S::About), "https://github.com/kewldan/freeflake");
}

/// A square pad: the dot's offset from the centre is the wind.
fn wind_pad(ui: &mut egui::Ui, wind: &mut Vec2) {
    let size = egui::vec2(120.0, 120.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    if let Some(p) = response.interact_pointer_pos() {
        let v = (p - rect.center()) / (rect.width() / 2.0);
        *wind = Vec2::new(v.x, v.y).clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
    }
    if response.double_clicked() {
        *wind = Vec2::ZERO;
    }
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 8.0, visuals.extreme_bg_color);
    let stroke = egui::Stroke::new(1.0, visuals.weak_text_color());
    painter.line_segment([rect.center_top(), rect.center_bottom()], stroke);
    painter.line_segment([rect.left_center(), rect.right_center()], stroke);
    let dot = rect.center() + egui::vec2(wind.x, wind.y) * rect.width() / 2.0;
    painter.arrow(
        rect.center(),
        dot - rect.center(),
        egui::Stroke::new(2.0, visuals.selection.bg_fill),
    );
    painter.circle_filled(dot, 6.0, visuals.selection.bg_fill);
}
