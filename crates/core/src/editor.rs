use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use egui::Color32;
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::*;
use nice_plug_egui::widgets::ParamSlider;
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, NiceEguiApp, RepaintNotifier, create_egui_editor,
};

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
        let app = self.editor.take()?;

        create_egui_editor(
            self.state.clone(),
            self.notifier.clone(),
            EguiNiceSettings::new()
                .with_tile("Lua Plugin Host")
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
    source: String,
    clicked: Vec<String>,
    gui_ctx: Option<GuiContext>,
    error: Option<String>,
}

impl LuaEditor {
    pub fn new(params: Arc<PluginParams>, kind: PluginKind) -> Self {
        let (source, loaded_script) = {
            let script = params.script.lock();
            (
                script.source.clone(),
                (script.name.clone(), script.source.clone()),
            )
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
            source,
            clicked: Vec::new(),
            gui_ctx: None,
            error,
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
        self.loaded_script = script.clone();
        self.source = script.1;
    }

    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Lua Plugin Host");
            ui.label(format!("({})", self.kind.as_str()));
            if ui.button("Reload script").clicked() {
                self.params.request_reload();
                self.error = None;
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

    fn render_script_editor(&mut self, ui: &mut egui::Ui) {
        let mut source = std::mem::take(&mut self.source);

        egui::CollapsingHeader::new("Script")
            .default_open(false)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut source)
                        .code_editor()
                        .desired_rows(14)
                        .desired_width(f32::INFINITY),
                );

                ui.horizontal(|ui| {
                    if ui.button("Apply & reload").clicked() {
                        self.params.set_script("user.lua", source.clone());
                        self.error = None;
                    }

                    if ui.button("Reset to default").clicked() {
                        source = DEFAULT_SCRIPT.to_string();
                        self.params.set_script("default.lua", source.clone());
                        self.error = None;
                    }

                    if ui.button("Reset macros to script defaults").clicked() {
                        self.reset_macros_to_script_defaults();
                    }
                });
            });

        self.source = source;
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
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut nice_plug_egui::Frame) {
        let Some(gui_ctx) = self.gui_ctx.clone() else {
            return;
        };
        let setter = gui_ctx.param_setter();

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
                self.render_script_editor(ui);
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
            UiCommand::Heading(text) => {
                ui.heading(text);
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
            UiCommand::Meter { value, label } => {
                ui.horizontal(|ui| {
                    if let Some(label) = label {
                        ui.label(label);
                    }
                    ui.add(
                        egui::ProgressBar::new(value.clamp(0.0, 1.0))
                            .desired_width(180.0)
                            .show_percentage(),
                    );
                });
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
