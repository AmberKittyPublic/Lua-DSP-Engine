use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use egui::Color32;
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::*;
use nice_plug_egui::widgets::ParamSlider;
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, NiceEguiApp, RepaintNotifier, create_egui_editor,
};

use crate::library;
use crate::lua::{HostRole, LuaHost};
use crate::params::{DEFAULT_SCRIPT, MacroScale, MacroScaleType, PluginKind, PluginParams};
use crate::ui::UiCommand;

const MIN_WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(760.0, 640.0);
const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(MIN_WINDOW_SIZE);

pub type LuaEditorType = EguiEditor<LuaEditor>;

pub struct EditorHost {
    state: Arc<EguiEditorState>,
    notifier: RepaintNotifier,
    editor: Option<LuaEditor>,
}

impl EditorHost {
    pub fn new(params: Arc<PluginParams>, kind: PluginKind) -> Self {
        Self {
            state: EguiEditorState::from_size(MIN_WINDOW_SIZE, 1.0),
            notifier: RepaintNotifier::new(),
            editor: Some(LuaEditor::new(params, kind)),
        }
    }

    pub fn create(&mut self) -> Option<LuaEditorType> {
        if let Some(editor) = &self.editor {
            editor.params.set_editor_open(true);
        }
        let app = self.editor.take()?;

        create_egui_editor(
            self.state.clone(),
            self.notifier.clone(),
            EguiNiceSettings::new()
                .with_tile("Lumina DSP")
                .with_resize_hint(RESIZE_HINT),
            app,
        )
    }

    pub fn is_open(&self) -> bool {
        self.state.is_open()
    }

    pub fn request_repaint(&self) {
        self.notifier.request_repaint();
    }
}

pub struct LuaEditor {
    params: Arc<PluginParams>,
    kind: PluginKind,
    host: Option<LuaHost>,
    loaded_version: u64,
    loaded_script: (String, String),
    script_dir: Option<PathBuf>,
    library_initialized: bool,
    pending_script_load: Option<mpsc::Receiver<Option<PathBuf>>>,
    clicked: Vec<String>,
    gui_ctx: Option<GuiContext>,
    error: Option<String>,
    library_error: Option<String>,
}

impl LuaEditor {
    pub fn new(params: Arc<PluginParams>, kind: PluginKind) -> Self {
        let loaded_script = {
            let script = params.script.lock();
            (script.name.clone(), script.source.clone())
        };
        let loaded_version = params.script_version.load(Ordering::Acquire);
        let host = LuaHost::new(params.clone(), kind, HostRole::Ui, params.sample_rate());
        let error = host.last_error().map(|error| error.to_string());

        Self {
            params,
            kind,
            host: Some(host),
            loaded_version,
            loaded_script,
            script_dir: library::extensions_dir(kind),
            library_initialized: false,
            pending_script_load: None,
            clicked: Vec::new(),
            gui_ctx: None,
            error,
            library_error: None,
        }
    }

    fn ensure_library(&mut self) {
        if self.library_initialized {
            return;
        }
        self.library_initialized = true;
        match library::install_factory_library() {
            Ok(root) => {
                self.script_dir = Some(
                    root.join(library::EXTENSIONS_FOLDER)
                        .join(self.kind.extension_folder_name()),
                );
                self.library_error = None;
            }
            Err(error) => self.library_error = Some(error),
        }
    }

    fn ensure_host(&mut self) {
        let version = self.params.script_version.load(Ordering::Acquire);
        let script = {
            let script = self.params.script.lock();
            (script.name.clone(), script.source.clone())
        };
        if self.host.is_some() && self.loaded_version == version && self.loaded_script == script {
            return;
        }

        let host = LuaHost::new(
            self.params.clone(),
            self.kind,
            HostRole::Ui,
            self.params.sample_rate(),
        );
        self.error = host.last_error().map(|error| error.to_string());
        self.host = Some(host);
        self.loaded_version = version;
        self.loaded_script = script;
    }

    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Lumina DSP");
            ui.label(format!("({})", self.kind.as_str()));
            if ui.button("Reload script").clicked() {
                self.params.request_reload();
                self.error = None;
            }

            if self.kind == PluginKind::Instrument {
                let mut oversampling = self.params.oversampling_factor() as i32;
                egui::ComboBox::from_label("Oversampling")
                    .selected_text(format!("{oversampling}x"))
                    .show_ui(ui, |ui| {
                        for factor in [1, 2, 4, 8, 16] {
                            ui.selectable_value(&mut oversampling, factor, format!("{factor}x"));
                        }
                    });
                self.params.set_oversampling_factor(oversampling as usize);
            }
        });

        let name = self.params.script.lock().name.clone();
        ui.label(format!("script: {name}"));
    }

    fn render_macros(&self, ui: &mut egui::Ui, setter: &ParamSetter<'_>) {
        egui::CollapsingHeader::new("All macros")
            .default_open(false)
            .show(ui, |ui| {
                egui::Grid::new("macro_grid")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for (index, slot) in self.params.macros.iter().enumerate() {
                            render_macro_label(
                                ui,
                                &format!("Macro {}", index + 1),
                                &slot.value,
                                self.params.script_macro_default_normalized(index),
                                setter,
                            );
                            render_macro_slider(
                                ui,
                                &slot.value,
                                self.params.macro_scale(index),
                                setter,
                                220.0,
                            );
                            ui.end_row();
                        }
                    });
            });
    }

    fn render_voice_settings(&self, ui: &mut egui::Ui, setter: &ParamSetter<'_>) {
        if self.kind != PluginKind::Instrument {
            return;
        }

        ui.horizontal(|ui| {
            ui.label("Voice mode");
            ui.add(ParamSlider::for_param(&self.params.voice_mode, setter).with_width(130.0));
            ui.label("Max voices");
            ui.add(ParamSlider::for_param(&self.params.max_voices, setter).with_width(100.0));
        });
    }

    fn poll_pending_script_load(&mut self) {
        let Some(receiver) = &self.pending_script_load else {
            return;
        };

        match receiver.try_recv() {
            Ok(Some(path)) => {
                self.pending_script_load = None;
                let is_manifest = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.eq_ignore_ascii_case("manifest.json"))
                    .unwrap_or(false);
                if is_manifest {
                    match library::read_extension(&path) {
                        Ok(package) => {
                            self.params.set_extension(
                                &path,
                                &package.manifest,
                                package.entry_source,
                                package.modules,
                            );
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                } else {
                    match library::read_script(&path) {
                        Ok(source) => {
                            self.params.set_script_file(&path, source);
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            Ok(None) => self.pending_script_load = None,
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending_script_load = None;
                self.error = Some("script file dialog failed".to_string());
            }
        }
    }

    fn render_script_controls(&mut self, ui: &mut egui::Ui) {
        self.poll_pending_script_load();
        egui::CollapsingHeader::new("Script")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let loading = self.pending_script_load.is_some();
                    if ui
                        .add_enabled(!loading, egui::Button::new("Load script..."))
                        .clicked()
                    {
                        self.start_script_load();
                    }

                    if ui.button("Reset to default").clicked() {
                        self.params
                            .set_script("default.lua", DEFAULT_SCRIPT.to_string());
                        self.error = None;
                    }

                    if ui.button("Reset macros to script defaults").clicked() {
                        self.reset_macros_to_script_defaults();
                    }
                });

                let (name, path) = {
                    let script = self.params.script.lock();
                    (script.name.clone(), script.path.clone())
                };
                ui.label(match path {
                    Some(path) => format!("loaded: {path}"),
                    None => format!("loaded: {name}"),
                });
                if let Some(script_dir) = &self.script_dir {
                    ui.label(format!("extensions folder: {}", script_dir.display()));
                }
                if let Some(error) = &self.library_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            });
    }

    fn start_script_load(&mut self) {
        let script_dir = self.script_dir.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Load script or extension")
                .add_filter("Lua scripts and extensions", &["lua", "json"]);
            if let Some(script_dir) = &script_dir {
                dialog = dialog.set_directory(script_dir);
            }
            let _ = sender.send(dialog.pick_file());
        });
        self.pending_script_load = Some(receiver);
    }

    fn reset_macros_to_script_defaults(&self) {
        let Some(gui_ctx) = &self.gui_ctx else {
            return;
        };
        let setter = gui_ctx.param_setter();

        for (index, slot) in self.params.macros.iter().enumerate() {
            setter.begin_set_parameter(&slot.value);
            setter.set_parameter_normalized(
                &slot.value,
                self.params.script_macro_default_normalized(index),
            );
            setter.end_set_parameter(&slot.value);
        }
    }
}

impl NiceEguiApp for LuaEditor {
    fn build(
        &mut self,
        _egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        self.gui_ctx = Some(nice_gui_ctx);
        Ok(())
    }

    fn editor_closed(&mut self) {
        self.gui_ctx = None;
        self.params.set_editor_open(false);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut nice_plug_egui::Frame) {
        if !self.params.editor_open() {
            return;
        }
        let Some(gui_ctx) = self.gui_ctx.clone() else {
            return;
        };
        let setter = gui_ctx.param_setter();

        self.ensure_library();
        self.ensure_host();

        let clicked = std::mem::take(&mut self.clicked);
        let commands = match self.host.as_mut() {
            Some(host) => match host.build_ui_frame(clicked) {
                Ok(commands) => {
                    self.error = None;
                    commands
                }
                Err(error) => {
                    self.error = Some(error);
                    Vec::new()
                }
            },
            None => Vec::new(),
        };

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.render_header(ui);
                self.render_voice_settings(ui, &setter);

                if let Some(error) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }

                ui.separator();
                render_commands(ui, &commands, &self.params, &setter, &mut self.clicked);

                ui.separator();
                self.render_macros(ui, &setter);

                ui.separator();
                self.render_script_controls(ui);

                ui.separator();
                egui::CollapsingHeader::new("Debug")
                    .default_open(false)
                    .show(ui, |ui| {
                        let mut enabled = self.params.debug_latency_enabled();
                        if ui
                            .checkbox(&mut enabled, "Measure latency (adds overhead)")
                            .changed()
                        {
                            self.params.set_debug_latency_enabled(enabled);
                        }
                        let block_size = self.params.block_size();
                        let sample_rate = self.params.sample_rate();
                        let estimated_ms = if sample_rate > 0.0 {
                            block_size as f32 / sample_rate * 1000.0
                        } else {
                            0.0
                        };
                        ui.label(format!("Block size: {block_size} samples"));
                        ui.label(format!("Sample rate: {sample_rate:.0} Hz"));
                        if self.params.debug_latency_enabled() {
                            ui.label(format!(
                                "Measured processing latency: {:.3} ms",
                                self.params.last_processing_latency_ms()
                            ));
                        } else {
                            ui.label(format!("Estimated block latency: {estimated_ms:.2} ms"));
                        }
                        ui.label(format!("Output peak: {:.3}", self.params.meter_peak()));
                    });
            });
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }
}

fn render_commands(
    ui: &mut egui::Ui,
    commands: &[UiCommand],
    params: &PluginParams,
    setter: &ParamSetter<'_>,
    clicked: &mut Vec<String>,
) {
    for command in commands {
        match command {
            UiCommand::Label(text) => {
                ui.label(text);
            }
            UiCommand::Heading { text, color } => {
                if let Some(color) = color {
                    let color = parse_color(color);
                    ui.label(egui::RichText::new(text).heading().strong().color(color));
                } else {
                    ui.heading(text);
                }
            }
            UiCommand::Separator => {
                ui.separator();
            }
            UiCommand::HorizontalSeparator => {
                ui.add(egui::Separator::default().horizontal());
            }
            UiCommand::VerticalSeparator => {
                ui.add(egui::Separator::default().vertical());
            }
            UiCommand::Space(pixels) => {
                ui.add_space(*pixels);
            }
            UiCommand::RadioParam {
                index,
                label,
                choices,
            } => {
                if let Some(slot) = index
                    .checked_sub(1)
                    .and_then(|index| params.macros.get(index))
                {
                    let macro_index = *index - 1;
                    let last_choice = choices.len().saturating_sub(1);
                    let selected = (slot.value.unmodulated_normalized_value() * last_choice as f32)
                        .round() as usize;
                    ui.horizontal_wrapped(|ui| {
                        render_macro_label(
                            ui,
                            label,
                            &slot.value,
                            params.script_macro_default(macro_index),
                            setter,
                        );
                        for (choice_index, choice) in choices.iter().enumerate() {
                            if ui
                                .add(egui::RadioButton::new(selected == choice_index, choice))
                                .clicked()
                            {
                                setter.begin_set_parameter(&slot.value);
                                setter.set_parameter_normalized(
                                    &slot.value,
                                    choice_index as f32 / last_choice as f32,
                                );
                                setter.end_set_parameter(&slot.value);
                            }
                        }
                    });
                }
            }
            UiCommand::Button {
                id,
                label,
                clicked: was_clicked,
            } => {
                let mut button = egui::Button::new(label);
                if *was_clicked {
                    button = button.fill(Color32::from_rgb(58, 92, 138));
                }

                if ui.add(button).clicked() {
                    clicked.push(id.clone());
                }
            }
            UiCommand::Param {
                index,
                label,
                scale,
            } => {
                if let Some(slot) = index
                    .checked_sub(1)
                    .and_then(|index| params.macros.get(index))
                {
                    let macro_index = *index - 1;
                    ui.horizontal(|ui| {
                        render_macro_label(
                            ui,
                            label,
                            &slot.value,
                            params.script_macro_default_normalized(macro_index),
                            setter,
                        );
                        render_macro_slider(ui, &slot.value, *scale, setter, 200.0);
                    });
                }
            }
            UiCommand::Knob {
                index,
                label,
                scale,
            } => {
                if let Some(slot) = index
                    .checked_sub(1)
                    .and_then(|index| params.macros.get(index))
                {
                    let macro_index = *index - 1;
                    render_macro_knob(
                        ui,
                        label,
                        &slot.value,
                        *scale,
                        params.script_macro_default_normalized(macro_index),
                        setter,
                    );
                }
            }
            UiCommand::Toggle { index, label } => {
                if let Some(slot) = index
                    .checked_sub(1)
                    .and_then(|index| params.macros.get(index))
                {
                    let mut checked = slot.value.unmodulated_normalized_value() >= 0.5;
                    if ui.checkbox(&mut checked, label).changed() {
                        setter.begin_set_parameter(&slot.value);
                        setter.set_parameter_normalized(&slot.value, f32::from(checked));
                        setter.end_set_parameter(&slot.value);
                    }
                }
            }
            UiCommand::Meter {
                value,
                label,
                color,
            } => {
                ui.horizontal(|ui| {
                    if let Some(label) = label {
                        ui.label(label);
                    }
                    let mut bar = egui::ProgressBar::new(value.clamp(0.0, 1.0))
                        .desired_width(180.0)
                        .show_percentage();
                    if let Some(color) = color {
                        bar = bar.fill(parse_color(color));
                    }
                    ui.add(bar);
                });
            }
            UiCommand::Graph { children } => {
                let size = egui::vec2(320.0, 120.0);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::GRAY),
                    egui::StrokeKind::Inside,
                );
                for child in children {
                    if let UiCommand::Plot { values, color } = child {
                        if values.is_empty() {
                            continue;
                        }
                        let color = color
                            .as_deref()
                            .map(parse_color)
                            .unwrap_or(egui::Color32::LIGHT_BLUE);
                        let mut points = Vec::with_capacity(values.len());
                        for (index, value) in values.iter().enumerate() {
                            let x = rect.left()
                                + index as f32 / (values.len().saturating_sub(1).max(1) as f32)
                                    * rect.width();
                            let y = rect.bottom() - value.clamp(0.0, 1.0) * rect.height();
                            points.push(egui::pos2(x, y));
                        }
                        painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, color)));
                    }
                }
                painter.line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                );
            }
            UiCommand::Plot { .. } => {}
            UiCommand::VerticalMeter { value } => {
                let size = egui::vec2(18.0, 90.0);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, egui::Color32::from_gray(30));
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::GRAY),
                    egui::StrokeKind::Inside,
                );
                let value = value.clamp(0.0, 1.0);
                let green_top = 0.6;
                let orange_top = 0.8;
                // segment heights
                let segments = [
                    (0.0f32, green_top, egui::Color32::GREEN),
                    (green_top, orange_top, egui::Color32::from_rgb(255, 165, 0)),
                    (orange_top, 1.0f32, egui::Color32::RED),
                ];
                for (start, end, color) in segments {
                    let seg_start = start.min(value);
                    let seg_end = end.min(value);
                    if seg_end > seg_start {
                        let bottom = rect.bottom() - seg_start * rect.height();
                        let top = rect.bottom() - seg_end * rect.height();
                        let seg_rect = egui::Rect::from_min_max(
                            egui::pos2(rect.left(), top),
                            egui::pos2(rect.right(), bottom),
                        );
                        painter.rect_filled(seg_rect, 0.0, color);
                    }
                }
            }
            UiCommand::LevelGraph => {
                let levels = params.level_trace();
                let gains = params.gain_trace();
                let size = egui::vec2(320.0, 120.0);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::GRAY),
                    egui::StrokeKind::Inside,
                );
                if !levels.is_empty() {
                    let mut points = Vec::with_capacity(levels.len());
                    for (index, value) in levels.iter().enumerate() {
                        let x = rect.left()
                            + index as f32 / (levels.len().saturating_sub(1).max(1) as f32)
                                * rect.width();
                        let y = rect.bottom() - value.clamp(0.0, 1.0) * rect.height();
                        points.push(egui::pos2(x, y));
                    }
                    painter.add(egui::Shape::line(
                        points,
                        egui::Stroke::new(1.5, egui::Color32::GREEN),
                    ));
                }
                if !gains.is_empty() {
                    let mut points = Vec::with_capacity(gains.len());
                    for (index, value) in gains.iter().enumerate() {
                        let x = rect.left()
                            + index as f32 / (gains.len().saturating_sub(1).max(1) as f32)
                                * rect.width();
                        let y = rect.bottom() - value.clamp(0.0, 1.0) * rect.height();
                        points.push(egui::pos2(x, y));
                    }
                    painter.add(egui::Shape::line(
                        points,
                        egui::Stroke::new(1.5, egui::Color32::RED),
                    ));
                }
                painter.line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                );
            }
            UiCommand::CompressionCurve {
                threshold,
                ratio,
                knee,
            } => {
                let size = egui::vec2(220.0, 120.0);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::GRAY),
                    egui::StrokeKind::Inside,
                );
                let mut points = Vec::with_capacity(129);
                for index in 0..=128 {
                    let input = index as f32 / 128.0;
                    let input_db = -60.0 + input * 60.0;
                    let over = (input_db - threshold).max(0.0);
                    let knee_width = knee.max(0.001);
                    let soft = if over < knee_width {
                        over * over / (2.0 * knee_width)
                    } else {
                        over - knee_width / 2.0
                    };
                    let reduction = soft * (1.0 - 1.0 / ratio.max(1.0));
                    let output_db = input_db - reduction;
                    let output = ((output_db + 60.0) / 60.0).clamp(0.0, 1.0);
                    let x = rect.left() + input * rect.width();
                    let y = rect.bottom() - output * rect.height();
                    points.push(egui::pos2(x, y));
                }
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 180, 0)),
                ));
                // unity line
                let unity = vec![
                    egui::pos2(rect.left(), rect.bottom()),
                    egui::pos2(rect.right(), rect.top()),
                ];
                painter.add(egui::Shape::line(
                    unity,
                    egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                ));
            }
            UiCommand::CompressorGraph {
                threshold,
                ratio,
                knee,
            } => {
                let levels = params.level_trace();
                let gains = params.gain_trace();
                let size = egui::vec2(320.0, 120.0);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::GRAY),
                    egui::StrokeKind::Inside,
                );
                if !levels.is_empty() {
                    let mut points = Vec::with_capacity(levels.len());
                    for (index, value) in levels.iter().enumerate() {
                        let x = rect.left()
                            + index as f32 / (levels.len().saturating_sub(1).max(1) as f32)
                                * rect.width();
                        let y = rect.bottom() - value.clamp(0.0, 1.0) * rect.height();
                        points.push(egui::pos2(x, y));
                    }
                    painter.add(egui::Shape::line(
                        points,
                        egui::Stroke::new(1.5, egui::Color32::GREEN),
                    ));
                }
                if !gains.is_empty() {
                    let mut points = Vec::with_capacity(gains.len());
                    for (index, value) in gains.iter().enumerate() {
                        let x = rect.left()
                            + index as f32 / (gains.len().saturating_sub(1).max(1) as f32)
                                * rect.width();
                        let y = rect.bottom() - value.clamp(0.0, 1.0) * rect.height();
                        points.push(egui::pos2(x, y));
                    }
                    painter.add(egui::Shape::line(
                        points,
                        egui::Stroke::new(1.5, egui::Color32::RED),
                    ));
                }
                // overlay compression curve
                let mut curve = Vec::with_capacity(129);
                for index in 0..=128 {
                    let input = index as f32 / 128.0;
                    let input_db = -60.0 + input * 60.0;
                    let over = (input_db - threshold).max(0.0);
                    let knee_width = knee.max(0.001);
                    let soft = if over < knee_width {
                        over * over / (2.0 * knee_width)
                    } else {
                        over - knee_width / 2.0
                    };
                    let reduction = soft * (1.0 - 1.0 / ratio.max(1.0));
                    let output_db = input_db - reduction;
                    let output = ((output_db + 60.0) / 60.0).clamp(0.0, 1.0);
                    let x = rect.left() + input * rect.width();
                    let y = rect.bottom() - output * rect.height();
                    curve.push(egui::pos2(x, y));
                }
                painter.add(egui::Shape::line(
                    curve,
                    egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(255, 180, 0, 160)),
                ));
                painter.line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                );
            }
            UiCommand::Row(children) => {
                ui.horizontal(|ui| {
                    render_commands(ui, children, params, setter, clicked);
                });
            }
            UiCommand::Column(children) => {
                ui.vertical(|ui| {
                    render_commands(ui, children, params, setter, clicked);
                });
            }
            UiCommand::Group { title, children } => {
                ui.group(|ui| {
                    if let Some(title) = title {
                        ui.label(egui::RichText::new(title).strong());
                    }
                    render_commands(ui, children, params, setter, clicked);
                });
            }
            UiCommand::Collapsing { title, children } => {
                egui::CollapsingHeader::new(title)
                    .default_open(true)
                    .show(ui, |ui| {
                        render_commands(ui, children, params, setter, clicked);
                    });
            }
        }
    }
}

fn render_macro_label(
    ui: &mut egui::Ui,
    label: &str,
    param: &FloatParam,
    default_value: f32,
    setter: &ParamSetter<'_>,
) {
    let response = ui
        .add(egui::Label::new(label).sense(egui::Sense::click()))
        .on_hover_text("Double-click to reset to the script default");
    if response.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, default_value);
        setter.end_set_parameter(param);
    }
}

fn render_macro_slider(
    ui: &mut egui::Ui,
    param: &FloatParam,
    scale: Option<MacroScale>,
    setter: &ParamSetter<'_>,
    width: f32,
) {
    let Some(scale) = scale else {
        ui.add(ParamSlider::for_param(param, setter).with_width(width));
        return;
    };

    let mut value = scale.value(param.unmodulated_normalized_value());
    let mut slider = egui::Slider::new(&mut value, scale.min..=scale.max);
    if scale.kind == MacroScaleType::Integer {
        slider = slider.integer();
    }
    if ui.add_sized([width, 20.0], slider).changed() {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, scale.normalize(value));
        setter.end_set_parameter(param);
    }
}

fn render_macro_knob(
    ui: &mut egui::Ui,
    label: &str,
    param: &FloatParam,
    scale: Option<MacroScale>,
    default_normalized: f32,
    setter: &ParamSetter<'_>,
) {
    let scale = scale.unwrap_or(MacroScale {
        kind: MacroScaleType::Float,
        min: 0.0,
        max: 1.0,
    });
    let size = 48.0;

    ui.vertical(|ui| {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click_and_drag());
        let mut normalized = param.unmodulated_normalized_value();
        if response.dragged() {
            let delta = ui.input(|input| input.pointer.delta());
            normalized = (normalized + (delta.x - delta.y) * 0.005).clamp(0.0, 1.0);
            normalized = scale.normalize(scale.value(normalized));
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, normalized);
            setter.end_set_parameter(param);
        }
        if response.double_clicked() {
            normalized = default_normalized;
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, default_normalized);
            setter.end_set_parameter(param);
        }

        let value = scale.value(normalized);
        let value_text = match scale.kind {
            MacroScaleType::Integer => format!("{value:.0}"),
            MacroScaleType::Float => format!("{value:.2}"),
        };
        let visuals = ui.style().interact(&response);
        let center = rect.center();
        let radius = size * 0.42;
        let painter = ui.painter();
        painter.circle_filled(center, radius, visuals.bg_fill);
        painter.circle_stroke(center, radius, visuals.bg_stroke);
        let angle =
            std::f32::consts::PI * 0.75 + normalized.clamp(0.0, 1.0) * std::f32::consts::PI * 1.5;
        let direction = egui::vec2(angle.cos(), angle.sin());
        painter.line_segment(
            [
                center + direction * radius * 0.3,
                center + direction * radius * 0.9,
            ],
            egui::Stroke::new(2.0, visuals.fg_stroke.color),
        );

        let label_response = ui
            .add(egui::Label::new(label).sense(egui::Sense::click()))
            .on_hover_text("Drag to adjust; double-click to reset to the script default");
        if label_response.double_clicked() {
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, default_normalized);
            setter.end_set_parameter(param);
        }
        ui.label(egui::RichText::new(value_text).small());
    });
}

fn parse_color(color: &str) -> Color32 {
    match color.to_ascii_lowercase().as_str() {
        "red" => Color32::RED,
        "green" => Color32::GREEN,
        "blue" => Color32::BLUE,
        "yellow" => Color32::YELLOW,
        "orange" => Color32::from_rgb(255, 165, 0),
        "white" => Color32::WHITE,
        "black" => Color32::BLACK,
        "gray" | "grey" => Color32::GRAY,
        _ => {
            let hex = color.trim_start_matches('#');
            if hex.len() == 6
                && let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(&hex[0..2], 16),
                    u8::from_str_radix(&hex[2..4], 16),
                    u8::from_str_radix(&hex[4..6], 16),
                )
            {
                return Color32::from_rgb(r, g, b);
            }
            Color32::RED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_host_reloads_when_restored_script_changes_without_version_bump() {
        let params = Arc::new(PluginParams::default());
        let mut editor = LuaEditor::new(params.clone(), PluginKind::Instrument);
        let version = params.script_version.load(Ordering::Acquire);
        {
            let mut script = params.script.lock();
            script.name = "restored.lua".to_string();
            script.source = "function draw() ui.label('Restored UI') end".to_string();
        }
        assert_eq!(params.script_version.load(Ordering::Acquire), version);

        editor.ensure_host();
        let commands = editor
            .host
            .as_mut()
            .expect("UI host should be recreated")
            .build_ui_frame(Vec::new())
            .expect("restored draw() should run");
        assert!(commands.iter().any(|command| matches!(
            command,
            UiCommand::Label(text) if text == "Restored UI"
        )));
    }
}
