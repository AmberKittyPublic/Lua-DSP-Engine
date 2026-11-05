use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use mlua::{Function, Lua, Table, Value, VmState};
use nice_plug::midi::{Channel, Key, VoiceID};
use nice_plug::prelude::*;
use parking_lot::Mutex;

use crate::audio::{AudioBridge, BlockAccess, MidiOut};
use crate::params::{
    MACRO_COUNT, MAX_VOICES, MacroScale, MacroScaleType, PluginKind, PluginParams,
};
use crate::synth::install_synth_api;
use crate::ui::{UiBuilder, UiCommand};
use crate::voice::{Voice, VoiceManager};

const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const LOAD_BUDGET: u32 = 4_000_000;
const PROCESS_BUDGET: u32 = 250_000;
const MIDI_BUDGET: u32 = 100_000;
const UI_BUDGET: u32 = 250_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRole {
    Audio,
    Ui,
}

impl HostRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Ui => "ui",
        }
    }
}

struct Budget {
    used: AtomicU32,
    limit: AtomicU32,
}

impl Budget {
    fn new(limit: u32) -> Arc<Self> {
        Arc::new(Self {
            used: AtomicU32::new(0),
            limit: AtomicU32::new(limit),
        })
    }

    fn reset(&self, limit: u32) {
        self.limit.store(limit, Ordering::Relaxed);
        self.used.store(0, Ordering::Relaxed);
    }

    fn check(&self) -> mlua::Result<VmState> {
        let used = self.used.fetch_add(1, Ordering::Relaxed) + 1;
        if used > self.limit.load(Ordering::Relaxed) {
            Err(mlua::Error::RuntimeError(
                "script instruction budget exceeded".to_string(),
            ))
        } else {
            Ok(VmState::Continue)
        }
    }
}

struct LoadedLua {
    lua: Lua,
    host_table: Table,
    process_fn: Option<Function>,
    process_voice_fn: Option<Function>,
    voice_ended_fn: Option<Function>,
    midi_fn: Option<Function>,
    ui_fn: Option<Function>,
    voice_tables: HashMap<u64, Table>,
    budget: Arc<Budget>,
}

/// Owns one Lua state. A plugin has two independent hosts: one on the audio
/// thread and one on the UI thread. They share parameters and meters through
/// atomics, never through Lua state.
pub struct LuaHost {
    params: Arc<PluginParams>,
    kind: PluginKind,
    role: HostRole,
    audio_bridge: Arc<AudioBridge>,
    voices: VoiceManager,
    midi_out: Arc<Mutex<MidiOut>>,
    ui_builder: Arc<Mutex<UiBuilder>>,
    loaded: Option<LoadedLua>,
    loaded_version: u64,
    last_error: Option<String>,
}

impl LuaHost {
    pub fn new(
        params: Arc<PluginParams>,
        kind: PluginKind,
        role: HostRole,
        sample_rate: f32,
    ) -> Self {
        let loaded_version = params.script_version.load(Ordering::Acquire);
        let mut host = Self {
            params,
            kind,
            role,
            audio_bridge: Arc::new(AudioBridge::new()),
            voices: VoiceManager::default(),
            midi_out: Arc::new(Mutex::new(MidiOut::default())),
            ui_builder: Arc::new(Mutex::new(UiBuilder::new())),
            loaded: None,
            loaded_version,
            last_error: None,
        };

        match host.load_script(sample_rate) {
            Ok(loaded) => host.loaded = Some(loaded),
            Err(err) => host.last_error = Some(err.to_string()),
        }

        host
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn has_script(&self) -> bool {
        self.loaded
            .as_ref()
            .map(|loaded| {
                loaded.process_fn.is_some()
                    || loaded.process_voice_fn.is_some()
                    || loaded.midi_fn.is_some()
                    || loaded.ui_fn.is_some()
            })
            .unwrap_or(false)
    }

    fn maybe_reload(&mut self) {
        let version = self.params.script_version.load(Ordering::Acquire);
        if version == self.loaded_version {
            return;
        }

        self.loaded_version = version;
        match self.load_script(self.params.sample_rate()) {
            Ok(loaded) => {
                self.loaded = Some(loaded);
                self.last_error = None;
            }
            Err(err) => {
                self.loaded = None;
                self.last_error = Some(err.to_string());
            }
        }
    }

    fn load_script(&self, sample_rate: f32) -> mlua::Result<LoadedLua> {
        let source = {
            let script = self.params.script.lock();
            (script.name.clone(), script.source.clone())
        };

        let lua = Lua::new();
        let _ = lua.set_memory_limit(MEMORY_LIMIT);

        let budget = Budget::new(LOAD_BUDGET);
        {
            let budget = budget.clone();
            lua.set_interrupt(move |_| budget.check());
        }

        let host_table = lua.create_table()?;
        host_table.set("kind", self.kind.as_str())?;
        host_table.set("role", self.role.as_str())?;
        host_table.set("script_name", source.0.clone())?;
        host_table.set("sample_rate", sample_rate)?;
        host_table.set("tempo", Value::Nil)?;
        host_table.set("playing", false)?;
        host_table.set("pos_beats", Value::Nil)?;
        host_table.set("num_samples", 0u64)?;
        host_table.set("voice_mode", self.params.effective_voice_mode().as_str())?;
        host_table.set("max_voices", self.params.effective_max_voices())?;

        let meter_params = self.params.clone();
        host_table.set(
            "meter",
            lua.create_function(move |_, ()| Ok(meter_params.meter_peak() as f64))?,
        )?;

        let globals = lua.globals();
        globals.set("host", &host_table)?;
        let defaults_open = install_params_api(&lua, &globals, self.params.clone())?;
        install_wave_api(&lua, &globals)?;
        install_synth_api(
            &lua,
            &globals,
            self.params.clone(),
            self.audio_bridge.clone(),
        )?;
        install_audio_api(&lua, &globals, self.audio_bridge.clone())?;
        install_midi_api(&lua, &globals, self.midi_out.clone())?;
        install_ui_api(&lua, &globals, self.ui_builder.clone(), self.params.clone())?;

        globals.set(
            "log",
            lua.create_function(|_, message: String| {
                eprintln!("[lua] {message}");
                Ok(())
            })?,
        )?;

        budget.reset(LOAD_BUDGET);
        if self.role == HostRole::Ui {
            self.params.clear_script_macro_defaults();
        }
        self.params.clear_macro_scales();
        self.params.set_voice_mode_override(None);
        self.params.set_max_voices_override(None);
        lua.load(source.1.as_str())
            .set_name(source.0.as_str())
            .exec()?;
        defaults_open.store(false, Ordering::Relaxed);

        let process_fn = globals.get::<Function>("process").ok();
        let process_voice_fn = globals.get::<Function>("process_voice").ok();
        let voice_ended_fn = globals.get::<Function>("voice_ended").ok();
        let midi_fn = globals.get::<Function>("on_midi").ok();
        let ui_fn = globals.get::<Function>("draw").ok();

        Ok(LoadedLua {
            lua,
            host_table,
            process_fn,
            process_voice_fn,
            voice_ended_fn,
            midi_fn,
            ui_fn,
            voice_tables: HashMap::with_capacity(32),
            budget,
        })
    }

    pub fn process_audio(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        sample_rate: f32,
        tempo: Option<f64>,
        playing: bool,
        pos_beats: Option<f64>,
    ) {
        self.maybe_reload();

        self.voices.configure(
            self.params.effective_voice_mode(),
            self.params.effective_max_voices(),
        );
        self.notify_voice_ends();

        let (process_fn, process_voice_fn, host_table, budget) = match self.loaded.as_ref() {
            Some(loaded) => (
                loaded.process_fn.clone(),
                loaded.process_voice_fn.clone(),
                loaded.host_table.clone(),
                loaded.budget.clone(),
            ),
            None => {
                if self.kind == PluginKind::Instrument {
                    for channel in buffer.as_slice() {
                        channel.fill(0.0);
                    }
                    self.params.set_meter_peak(0.0);
                }
                return;
            }
        };

        if process_fn.is_none()
            && (self.kind != PluginKind::Instrument || process_voice_fn.is_none())
        {
            if self.kind == PluginKind::Instrument {
                for channel in buffer.as_slice() {
                    channel.fill(0.0);
                }
                self.params.set_meter_peak(0.0);
            }
            return;
        }

        let num_samples = buffer.samples();
        let _ = host_table.set("sample_rate", sample_rate);
        let _ = host_table.set("num_samples", num_samples as u64);
        let _ = host_table.set("tempo", tempo);
        let _ = host_table.set("playing", playing);
        let _ = host_table.set("pos_beats", pos_beats);
        let _ = host_table.set("voice_mode", self.params.effective_voice_mode().as_str());
        let _ = host_table.set("max_voices", self.params.effective_max_voices());

        let mut access = unsafe { BlockAccess::new(buffer, aux) };
        if self.kind == PluginKind::Instrument {
            access.clear();
        }
        self.audio_bridge.publish(&mut access);

        let mut error = None;
        if let Some(process_voice_fn) =
            process_voice_fn.filter(|_| self.kind == PluginKind::Instrument)
        {
            let voices = std::array::from_fn::<_, MAX_VOICES, _>(|index| {
                self.voices.voices().get(index).copied()
            });
            for voice in voices.into_iter().flatten() {
                let table = match self.loaded.as_mut() {
                    Some(loaded) => match loaded.voice_tables.get(&voice.id) {
                        Some(table) => table.clone(),
                        None => match loaded.lua.create_table() {
                            Ok(table) => {
                                loaded.voice_tables.insert(voice.id, table.clone());
                                table
                            }
                            Err(err) => {
                                error = Some(err.to_string());
                                break;
                            }
                        },
                    },
                    None => break,
                };

                if let Err(err) = populate_voice_table(&table, voice) {
                    error = Some(err.to_string());
                    break;
                }

                budget.reset(PROCESS_BUDGET);
                match process_voice_fn.call::<Value>(table) {
                    Ok(Value::Boolean(true)) => self.voices.finish(voice.id),
                    Ok(_) => {}
                    Err(err) => {
                        error = Some(err.to_string());
                        break;
                    }
                }
            }
        } else if let Some(process_fn) = process_fn {
            budget.reset(PROCESS_BUDGET);
            if let Err(err) = process_fn.call::<()>((
                num_samples as f64,
                sample_rate as f64,
                tempo,
                playing,
                pos_beats,
            )) {
                error = Some(err.to_string());
            }
        }

        self.notify_voice_ends();
        let voices = &self.voices;
        if let Some(loaded) = self.loaded.as_mut() {
            loaded.voice_tables.retain(|id, _| voices.contains(*id));
        }
        let peak = access.peak();
        self.audio_bridge.clear();

        match error {
            Some(error) => self.last_error = Some(error),
            None => {
                self.last_error = None;
                self.params.set_meter_peak(peak);
            }
        }
    }

    pub fn dispatch_midi<P: Plugin<SysExMessage = ()>>(
        &mut self,
        context: &mut impl ProcessContext<P>,
    ) {
        self.maybe_reload();

        self.voices.configure(
            self.params.effective_voice_mode(),
            self.params.effective_max_voices(),
        );
        self.notify_voice_ends();

        let (midi_fn, budget) = self
            .loaded
            .as_ref()
            .map(|loaded| (loaded.midi_fn.clone(), Some(loaded.budget.clone())))
            .unwrap_or((None, None));

        self.midi_out.lock().clear();

        let mut failed = false;
        while let Some(event) = context.next_event() {
            if self.kind == PluginKind::Instrument {
                self.voices.configure(
                    self.params.effective_voice_mode(),
                    self.params.effective_max_voices(),
                );
                self.voices.handle_event(&event);
                self.notify_voice_ends();
            }

            let (Some(midi_fn), Some(budget)) = (midi_fn.as_ref(), budget.as_ref()) else {
                continue;
            };
            if failed {
                continue;
            }

            let Some((kind, a, b, c, d)) = event_args(&event) else {
                continue;
            };

            budget.reset(MIDI_BUDGET);
            if let Err(err) = midi_fn.call::<()>((kind, a, b, c, d)) {
                self.last_error = Some(err.to_string());
                failed = true;
            }
        }
    }

    fn notify_voice_ends(&mut self) {
        let mut ended = [None; MAX_VOICES];
        let count = self.voices.take_terminated(&mut ended);
        let Some((callback, budget)) = self.loaded.as_ref().and_then(|loaded| {
            loaded
                .voice_ended_fn
                .clone()
                .map(|callback| (callback, loaded.budget.clone()))
        }) else {
            return;
        };

        for id in ended.into_iter().take(count).flatten() {
            budget.reset(MIDI_BUDGET);
            if let Err(err) = callback.call::<()>((id,)) {
                self.last_error = Some(err.to_string());
                break;
            }
        }
    }

    pub fn process_midi_block(
        &mut self,
        num_samples: usize,
        sample_rate: f32,
        tempo: Option<f64>,
        playing: bool,
        pos_beats: Option<f64>,
    ) {
        self.maybe_reload();

        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        let (Some(process_fn), host_table, budget) = (
            loaded.process_fn.clone(),
            loaded.host_table.clone(),
            loaded.budget.clone(),
        ) else {
            return;
        };

        let _ = host_table.set("sample_rate", sample_rate);
        let _ = host_table.set("num_samples", num_samples as u64);
        let _ = host_table.set("tempo", tempo);
        let _ = host_table.set("playing", playing);
        let _ = host_table.set("pos_beats", pos_beats);

        budget.reset(PROCESS_BUDGET);
        match process_fn.call::<()>((
            num_samples as f64,
            sample_rate as f64,
            tempo,
            playing,
            pos_beats,
        )) {
            Ok(()) => self.last_error = None,
            Err(err) => self.last_error = Some(err.to_string()),
        }
    }

    pub fn flush_midi<P: Plugin<SysExMessage = ()>>(
        &mut self,
        context: &mut impl ProcessContext<P>,
    ) {
        let mut events = self.midi_out.lock();
        for event in events.drain() {
            let _ = context.try_send_event(event);
        }
    }

    pub fn build_ui_frame(&mut self, clicked: Vec<String>) -> Result<Vec<UiCommand>, String> {
        self.maybe_reload();

        let (ui_fn, budget, host_table) = match self.loaded.as_ref() {
            Some(loaded) => (
                loaded.ui_fn.clone(),
                loaded.budget.clone(),
                loaded.host_table.clone(),
            ),
            None => {
                return Err(self
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "script failed to load".to_string()));
            }
        };

        let Some(ui_fn) = ui_fn else {
            return Err("script does not define draw()".to_string());
        };

        let _ = host_table.set("sample_rate", self.params.sample_rate());
        let _ = host_table.set("voice_mode", self.params.effective_voice_mode().as_str());
        let _ = host_table.set("max_voices", self.params.effective_max_voices());
        self.ui_builder.lock().begin(clicked);
        budget.reset(UI_BUDGET);
        let result = ui_fn.call::<()>(());
        let commands = self.ui_builder.lock().finish();

        match result {
            Ok(()) => {
                self.last_error = None;
                commands.map_err(|err| err.to_string())
            }
            Err(err) => {
                let message = err.to_string();
                self.last_error = Some(message.clone());
                Err(message)
            }
        }
    }
}

fn install_params_api(
    lua: &Lua,
    globals: &Table,
    params: Arc<PluginParams>,
) -> mlua::Result<Arc<AtomicBool>> {
    let table = lua.create_table()?;

    table.set("count", lua.create_function(move |_, ()| Ok(MACRO_COUNT))?)?;

    let values = params.clone();
    table.set(
        "get",
        lua.create_function(move |_, index: usize| {
            let zero_based = index.checked_sub(1).filter(|index| *index < MACRO_COUNT);
            let Some(zero_based) = zero_based else {
                return Err(mlua::Error::RuntimeError(format!(
                    "parameter index out of range: {index}"
                )));
            };

            Ok(values.macro_value(zero_based) as f64)
        })?,
    )?;

    let values = params.clone();
    table.set(
        "set_range",
        lua.create_function(move |_, (index, spec): (usize, Table)| {
            let zero_based = index.checked_sub(1).filter(|index| *index < MACRO_COUNT);
            let Some(zero_based) = zero_based else {
                return Err(mlua::Error::RuntimeError(format!(
                    "parameter index out of range: {index}"
                )));
            };
            values.set_macro_scale(zero_based, parse_macro_scale(&spec)?);
            Ok(())
        })?,
    )?;

    let values = params.clone();
    table.set(
        "default",
        lua.create_function(move |_, index: usize| {
            let zero_based = index.checked_sub(1).filter(|index| *index < MACRO_COUNT);
            let Some(zero_based) = zero_based else {
                return Err(mlua::Error::RuntimeError(format!(
                    "parameter index out of range: {index}"
                )));
            };

            Ok(values.script_macro_default(zero_based) as f64)
        })?,
    )?;

    let defaults_open = Arc::new(AtomicBool::new(true));
    let values = params.clone();
    let open = defaults_open.clone();
    table.set(
        "set_default",
        lua.create_function(move |_, (index, value): (usize, f32)| {
            if open.load(Ordering::Relaxed) {
                set_script_macro_default(&values, index, value)?;
            }
            Ok(())
        })?,
    )?;

    let values = params;
    let open = defaults_open.clone();
    table.set(
        "set_defaults",
        lua.create_function(move |_, defaults: Table| {
            if open.load(Ordering::Relaxed) {
                for pair in defaults.pairs::<usize, f32>() {
                    let (index, value) = pair?;
                    set_script_macro_default(&values, index, value)?;
                }
            }
            Ok(())
        })?,
    )?;

    globals.set("params", table)?;
    Ok(defaults_open)
}

fn set_script_macro_default(params: &PluginParams, index: usize, value: f32) -> mlua::Result<()> {
    let zero_based = index.checked_sub(1).filter(|index| *index < MACRO_COUNT);
    let Some(zero_based) = zero_based else {
        return Err(mlua::Error::RuntimeError(format!(
            "parameter index out of range: {index}"
        )));
    };
    if !value.is_finite() {
        return Err(mlua::Error::RuntimeError(format!(
            "macro default must be finite: {value}"
        )));
    }
    params.set_script_macro_default(zero_based, value);
    Ok(())
}

fn parse_macro_scale(spec: &Table) -> mlua::Result<MacroScale> {
    let type_name = spec
        .get::<Option<String>>("type")?
        .or(spec.get::<Option<String>>(1)?)
        .ok_or_else(|| mlua::Error::RuntimeError("macro scale must specify a type".to_string()))?;
    let min = spec
        .get::<Option<f32>>("min")?
        .or(spec.get::<Option<f32>>(2)?)
        .ok_or_else(|| mlua::Error::RuntimeError("macro scale must specify min".to_string()))?;
    let max = spec
        .get::<Option<f32>>("max")?
        .or(spec.get::<Option<f32>>(3)?)
        .ok_or_else(|| mlua::Error::RuntimeError("macro scale must specify max".to_string()))?;
    let kind = match type_name.as_str() {
        "float" => MacroScaleType::Float,
        "int" | "integer" => MacroScaleType::Integer,
        _ => {
            return Err(mlua::Error::RuntimeError(format!(
                "unsupported macro scale type: {type_name}"
            )));
        }
    };

    MacroScale::new(kind, min, max).ok_or_else(|| {
        mlua::Error::RuntimeError(
            "macro scale requires finite min < max; integer bounds must be whole numbers"
                .to_string(),
        )
    })
}

fn populate_voice_table(table: &Table, voice: Voice) -> mlua::Result<()> {
    table.set("id", voice.id)?;
    table.set("note", voice.note)?;
    table.set("channel", voice.channel)?;
    table.set("velocity", voice.velocity)?;
    table.set("gate", voice.gate)?;
    table.set(
        "frequency",
        440.0 * 2.0_f64.powf((voice.note as f64 - 69.0) / 12.0),
    )?;
    table.set("age", voice.age)?;
    Ok(())
}

fn install_wave_api(lua: &Lua, globals: &Table) -> mlua::Result<()> {
    let table = lua.create_table()?;

    table.set(
        "sine",
        lua.create_function(|_, phase: f64| {
            Ok((phase.rem_euclid(1.0) * std::f64::consts::TAU).sin())
        })?,
    )?;

    table.set(
        "sin",
        lua.create_function(|_, phase: f64| {
            Ok((phase.rem_euclid(1.0) * std::f64::consts::TAU).sin())
        })?,
    )?;

    table.set(
        "saw",
        lua.create_function(|_, phase: f64| Ok(phase.rem_euclid(1.0) * 2.0 - 1.0))?,
    )?;

    table.set(
        "square",
        lua.create_function(|_, phase: f64| {
            Ok(if phase.rem_euclid(1.0) < 0.5 {
                1.0
            } else {
                -1.0
            })
        })?,
    )?;

    table.set(
        "triangle",
        lua.create_function(|_, phase: f64| {
            let phase = phase.rem_euclid(1.0);
            Ok(1.0 - 4.0 * (phase - 0.5).abs())
        })?,
    )?;

    table.set(
        "pulse",
        lua.create_function(|_, (phase, width): (f64, Option<f64>)| {
            let width = width.unwrap_or(0.5).clamp(0.0, 1.0);
            Ok(if phase.rem_euclid(1.0) < width {
                1.0
            } else {
                -1.0
            })
        })?,
    )?;

    let noise_seed = Arc::new(AtomicU32::new(0x1234_5678));
    table.set(
        "noise",
        lua.create_function(move |_, ()| {
            let mut value = noise_seed.load(Ordering::Relaxed);
            value ^= value << 13;
            value ^= value >> 17;
            value ^= value << 5;
            noise_seed.store(value, Ordering::Relaxed);
            Ok((value as f64 / u32::MAX as f64) * 2.0 - 1.0)
        })?,
    )?;

    table.set(
        "fm",
        lua.create_function(|_, (carrier_phase, modulator, index): (f64, f64, f64)| {
            Ok(((carrier_phase + modulator * index / std::f64::consts::TAU)
                * std::f64::consts::TAU)
                .sin())
        })?,
    )?;

    table.set(
        "pd",
        lua.create_function(|_, (phase, amount): (f64, f64)| {
            let phase = phase.rem_euclid(1.0);
            let phase = (phase
                + amount.clamp(-0.95, 0.95) * (phase * std::f64::consts::TAU).sin()
                    / std::f64::consts::TAU)
                .rem_euclid(1.0);
            Ok((phase * std::f64::consts::TAU).sin())
        })?,
    )?;

    table.set(
        "rm",
        lua.create_function(|_, (carrier, modulator): (f64, f64)| Ok(carrier * modulator))?,
    )?;

    table.set(
        "am",
        lua.create_function(|_, (carrier, modulator, depth): (f64, f64, Option<f64>)| {
            let depth = depth.unwrap_or(1.0).clamp(0.0, 1.0);
            Ok(carrier * (1.0 - depth + depth * (modulator * 0.5 + 0.5)))
        })?,
    )?;

    globals.set("wave", table)?;
    Ok(())
}

fn install_audio_api(lua: &Lua, globals: &Table, bridge: Arc<AudioBridge>) -> mlua::Result<()> {
    let table = lua.create_table()?;

    let current = bridge.clone();
    table.set(
        "num_samples",
        lua.create_function(move |_, ()| Ok(current.num_samples()))?,
    )?;

    let current = bridge.clone();
    table.set(
        "num_channels",
        lua.create_function(move |_, ()| Ok(current.num_channels()))?,
    )?;

    let current = bridge.clone();
    table.set(
        "sc_num_channels",
        lua.create_function(move |_, ()| Ok(current.aux_channels()))?,
    )?;

    let current = bridge.clone();
    table.set(
        "get",
        lua.create_function(move |_, (channel, sample): (usize, usize)| {
            Ok(current
                .with_access(|access| access.get(channel, sample))
                .flatten())
        })?,
    )?;

    let current = bridge.clone();
    table.set(
        "set",
        lua.create_function(move |_, (channel, sample, value): (usize, usize, f32)| {
            current.with_access(|access| access.set(channel, sample, value));
            Ok(())
        })?,
    )?;

    let current = bridge.clone();
    table.set(
        "add",
        lua.create_function(move |_, (channel, sample, value): (usize, usize, f32)| {
            current.with_access(|access| access.add(channel, sample, value));
            Ok(())
        })?,
    )?;

    let current = bridge.clone();
    table.set(
        "gain",
        lua.create_function(move |_, (channel, gain): (usize, f32)| {
            current.with_access(|access| access.gain(channel, gain));
            Ok(())
        })?,
    )?;

    let current = bridge;
    table.set(
        "sc_get",
        lua.create_function(move |_, (channel, sample): (usize, usize)| {
            Ok(current
                .with_access(|access| access.sc_get(channel, sample))
                .flatten())
        })?,
    )?;

    globals.set("audio", table)?;
    Ok(())
}

fn install_midi_api(lua: &Lua, globals: &Table, midi_out: Arc<Mutex<MidiOut>>) -> mlua::Result<()> {
    let table = lua.create_table()?;

    let out = midi_out.clone();
    table.set(
        "send_note_on",
        lua.create_function(
            move |_,
                  (key, velocity, channel, timing, voice_id): (
                u8,
                f32,
                Option<u8>,
                Option<u32>,
                Option<i32>,
            )| {
                let voice_id = match voice_id {
                    Some(id) if id >= 0 => VoiceID::ID(id),
                    Some(_) => {
                        return Err(mlua::Error::RuntimeError(
                            "MIDI voice ID must be non-negative".to_string(),
                        ));
                    }
                    None => VoiceID::Wildcard,
                };
                out.lock().push(NoteEvent::NoteOn {
                    timing: timing.unwrap_or(0),
                    voice_id,
                    channel: channel_value(channel),
                    key: Key::Number(key),
                    velocity,
                });
                Ok(())
            },
        )?,
    )?;

    let out = midi_out.clone();
    table.set(
        "send_note_off",
        lua.create_function(
            move |_, (key, channel, timing, voice_id): (u8, Option<u8>, Option<u32>, Option<i32>)| {
                let voice_id = match voice_id {
                    Some(id) if id >= 0 => VoiceID::ID(id),
                    Some(_) => {
                        return Err(mlua::Error::RuntimeError(
                            "MIDI voice ID must be non-negative".to_string(),
                        ));
                    }
                    None => VoiceID::Wildcard,
                };
                out.lock().push(NoteEvent::NoteOff {
                    timing: timing.unwrap_or(0),
                    voice_id,
                    channel: channel_value(channel),
                    key: Key::Number(key),
                    velocity: 0.0,
                });
                Ok(())
            },
        )?,
    )?;

    let out = midi_out.clone();
    table.set(
        "send_cc",
        lua.create_function(
            move |_, (cc, value, channel, timing): (u8, f32, Option<u8>, Option<u32>)| {
                out.lock().push(NoteEvent::MidiCC {
                    timing: timing.unwrap_or(0),
                    channel: channel.unwrap_or(0).min(15),
                    cc,
                    value,
                });
                Ok(())
            },
        )?,
    )?;

    let out = midi_out.clone();
    table.set(
        "send_pitch_bend",
        lua.create_function(
            move |_, (value, channel, timing): (f32, Option<u8>, Option<u32>)| {
                out.lock().push(NoteEvent::MidiPitchBend {
                    timing: timing.unwrap_or(0),
                    channel: channel.unwrap_or(0).min(15),
                    value,
                });
                Ok(())
            },
        )?,
    )?;

    let out = midi_out;
    table.set(
        "send_channel_pressure",
        lua.create_function(
            move |_, (pressure, channel, timing): (f32, Option<u8>, Option<u32>)| {
                out.lock().push(NoteEvent::MidiChannelPressure {
                    timing: timing.unwrap_or(0),
                    channel: channel.unwrap_or(0).min(15),
                    pressure,
                });
                Ok(())
            },
        )?,
    )?;

    globals.set("midi", table)?;
    Ok(())
}

fn install_ui_api(
    lua: &Lua,
    globals: &Table,
    builder: Arc<Mutex<UiBuilder>>,
    params: Arc<PluginParams>,
) -> mlua::Result<()> {
    let table = lua.create_table()?;

    let ui = builder.clone();
    table.set(
        "label",
        lua.create_function(move |_, text: String| {
            ui.lock().label(text);
            Ok(())
        })?,
    )?;

    let ui = builder.clone();
    table.set(
        "heading",
        lua.create_function(move |_, text: String| {
            ui.lock().heading(text);
            Ok(())
        })?,
    )?;

    let ui = builder.clone();
    table.set(
        "separator",
        lua.create_function(move |_, ()| {
            ui.lock().separator();
            Ok(())
        })?,
    )?;

    let ui = builder.clone();
    table.set(
        "separator_horizontal",
        lua.create_function(move |_, ()| {
            ui.lock().horizontal_separator();
            Ok(())
        })?,
    )?;

    let ui = builder.clone();
    table.set(
        "separator_vertical",
        lua.create_function(move |_, ()| {
            ui.lock().vertical_separator();
            Ok(())
        })?,
    )?;

    let ui = builder.clone();
    table.set(
        "space",
        lua.create_function(move |_, pixels: Option<f32>| {
            ui.lock().space(pixels.unwrap_or(8.0));
            Ok(())
        })?,
    )?;

    let ui = builder.clone();
    table.set(
        "button",
        lua.create_function(move |_, (id, label): (String, String)| {
            ui.lock().button(id, label);
            Ok(())
        })?,
    )?;

    let ui = builder.clone();
    let macro_params = params;
    table.set(
        "param",
        lua.create_function(
            move |_, (index, label, spec): (usize, Option<String>, Option<Table>)| {
                let zero_based = index.checked_sub(1).filter(|index| *index < MACRO_COUNT);
                let Some(zero_based) = zero_based else {
                    return Err(mlua::Error::RuntimeError(format!(
                        "parameter index out of range: {index}"
                    )));
                };

                let scale = if let Some(spec) = spec {
                    let scale = parse_macro_scale(&spec)?;
                    macro_params.set_macro_scale(zero_based, scale);
                    Some(scale)
                } else {
                    macro_params.macro_scale(zero_based)
                };

                ui.lock().param(
                    index,
                    label.unwrap_or_else(|| format!("Macro {index}")),
                    scale,
                );
                Ok(())
            },
        )?,
    )?;

    let ui = builder.clone();
    table.set(
        "radio_param",
        lua.create_function(move |_, (index, label, choices): (usize, String, Table)| {
            if index == 0 || index > MACRO_COUNT {
                return Err(mlua::Error::RuntimeError(format!(
                    "parameter index out of range: {index}"
                )));
            }

            let choices = choices
                .sequence_values::<String>()
                .collect::<mlua::Result<Vec<_>>>()?;
            if choices.len() < 2 {
                return Err(mlua::Error::RuntimeError(
                    "ui.radio_param() requires at least two choices".to_string(),
                ));
            }

            ui.lock().radio_param(index, label, choices);
            Ok(())
        })?,
    )?;

    let ui = builder.clone();
    table.set(
        "meter",
        lua.create_function(move |_, (value, label): (f32, Option<String>)| {
            ui.lock().meter(value, label);
            Ok(())
        })?,
    )?;

    let ui = builder.clone();
    table.set(
        "row",
        lua.create_function(move |_, f: Function| {
            ui.lock().begin_row();
            let result = f.call::<()>(());
            let end = ui
                .lock()
                .end_row()
                .map_err(|error| mlua::Error::RuntimeError(error.to_string()));
            result.and(end)
        })?,
    )?;

    let ui = builder;
    table.set(
        "column",
        lua.create_function(move |_, f: Function| {
            ui.lock().begin_column();
            let result = f.call::<()>(());
            let end = ui
                .lock()
                .end_column()
                .map_err(|error| mlua::Error::RuntimeError(error.to_string()));
            result.and(end)
        })?,
    )?;

    globals.set("ui", table)?;
    Ok(())
}

fn channel_value(channel: Option<u8>) -> Channel {
    match channel {
        Some(channel) => Channel::Number(channel.min(15)),
        None => Channel::Wildcard,
    }
}

fn event_args<S>(event: &NoteEvent<S>) -> Option<(&'static str, f64, f64, f64, f64)> {
    const NO_VALUE: f64 = 0.0;

    match event {
        NoteEvent::NoteOn {
            timing,
            channel,
            key,
            velocity,
            ..
        } => Some((
            "note_on",
            key_number(*key) as f64,
            *velocity as f64,
            channel_number(*channel) as f64,
            *timing as f64,
        )),
        NoteEvent::NoteOff {
            timing,
            channel,
            key,
            velocity,
            ..
        } => Some((
            "note_off",
            key_number(*key) as f64,
            *velocity as f64,
            channel_number(*channel) as f64,
            *timing as f64,
        )),
        NoteEvent::Choke {
            timing,
            channel,
            key,
            ..
        } => Some((
            "choke",
            key_number(*key) as f64,
            NO_VALUE,
            channel_number(*channel) as f64,
            *timing as f64,
        )),
        NoteEvent::MidiCC {
            timing,
            channel,
            cc,
            value,
        } => Some((
            "cc",
            *cc as f64,
            *value as f64,
            *channel as f64,
            *timing as f64,
        )),
        NoteEvent::MidiPitchBend {
            timing,
            channel,
            value,
        } => Some((
            "pitch_bend",
            *value as f64,
            NO_VALUE,
            *channel as f64,
            *timing as f64,
        )),
        NoteEvent::MidiChannelPressure {
            timing,
            channel,
            pressure,
        } => Some((
            "channel_pressure",
            *pressure as f64,
            NO_VALUE,
            *channel as f64,
            *timing as f64,
        )),
        _ => None,
    }
}

fn channel_number(channel: Channel) -> u8 {
    match channel {
        Channel::Wildcard => 0,
        Channel::Number(channel) => channel,
    }
}

fn key_number(key: Key) -> u8 {
    match key {
        Key::Wildcard => 0,
        Key::Number(key) => key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_command(commands: &[UiCommand], predicate: impl Fn(&UiCommand) -> bool + Copy) -> bool {
        commands.iter().any(|command| {
            predicate(command)
                || match command {
                    UiCommand::Row(children) | UiCommand::Column(children) => {
                        has_command(children, predicate)
                    }
                    _ => false,
                }
        })
    }

    #[test]
    fn wave_api_matches_expected_shapes() {
        let lua = Lua::new();
        install_wave_api(&lua, &lua.globals()).expect("install wave api");

        lua.load(
            r#"
            assert(math.abs(wave.sine(0.25) - 1.0) < 1e-9)
            assert(math.abs(wave.sin(0.5)) < 1e-9)
            assert(math.abs(wave.saw(0.75) - 0.5) < 1e-9)
            assert(math.abs(wave.saw(-0.25) - 0.5) < 1e-9)
            assert(wave.square(0.25) == 1.0)
            assert(wave.square(0.75) == -1.0)
            assert(wave.triangle(0.0) == -1.0)
            assert(wave.triangle(0.5) == 1.0)
            assert(wave.pulse(0.25) == 1.0)
            assert(wave.pulse(0.75) == -1.0)
            assert(wave.pulse(0.75, 0.9) == 1.0)
            assert(math.abs(wave.fm(0.25, 0.0, 1.0) - 1.0) < 1e-9)
            assert(wave.rm(0.5, -0.5) == -0.25)
            assert(wave.am(1.0, 1.0) == 1.0)
            local n = wave.noise()
            assert(n >= -1.0 and n <= 1.0)
            "#,
        )
        .exec()
        .expect("wave assertions failed");
    }

    #[test]
    fn midi_voice_ids_are_optional_but_can_be_matched_explicitly() {
        let lua = Lua::new();
        let output = Arc::new(Mutex::new(MidiOut::default()));
        install_midi_api(&lua, &lua.globals(), output.clone()).expect("install MIDI API");

        lua.load(
            r#"
            midi.send_note_on(60, 1.0)
            midi.send_note_off(60)
            midi.send_note_on(61, 0.8, 0, 12, 7)
            midi.send_note_off(61, 0, 24, 7)
            "#,
        )
        .exec()
        .expect("MIDI voice ID calls failed");

        let events: Vec<_> = output.lock().drain().collect();
        assert!(matches!(
            events[0],
            NoteEvent::NoteOn {
                voice_id: VoiceID::Wildcard,
                ..
            }
        ));
        assert!(matches!(
            events[1],
            NoteEvent::NoteOff {
                voice_id: VoiceID::Wildcard,
                ..
            }
        ));
        assert!(matches!(
            events[2],
            NoteEvent::NoteOn {
                voice_id: VoiceID::ID(7),
                ..
            }
        ));
        assert!(matches!(
            events[3],
            NoteEvent::NoteOff {
                voice_id: VoiceID::ID(7),
                ..
            }
        ));
    }

    #[test]
    fn script_macro_defaults_are_captured_at_load_not_during_draw() {
        let params = Arc::new(PluginParams::default());
        params.set_script(
            "macro-defaults.lua",
            r#"
            params.set_defaults({ [1] = 0.75, [64] = 0.25 })
            function draw()
                params.set_default(1, 0.1)
            end
            "#,
        );
        let mut host = LuaHost::new(params.clone(), PluginKind::AudioFx, HostRole::Ui, 44_100.0);

        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        assert_eq!(params.script_macro_default(0), 0.75);
        assert_eq!(params.script_macro_default(63), 0.25);
        assert_eq!(params.macros[0].value.value(), 0.0);

        host.build_ui_frame(Vec::new()).expect("draw() failed");
        assert_eq!(params.script_macro_default(0), 0.75);
    }

    #[test]
    fn typed_macro_range_maps_getter_and_custom_slider() {
        let params = Arc::new(PluginParams::default());
        params.set_script(
            "typed-macro.lua",
            r#"
            local unison = { "int", 1, 16 }
            local detune = { type = "float", min = 0, max = 100 }
            params.set_range(1, unison)
            params.set_range(2, detune)
            params.set_defaults({ [1] = 4, [2] = 8 })
            function draw()
                ui.param(1, "Unison", unison)
                ui.param(2, "Detune", detune)
                ui.label("Mapped count: " .. params.get(1))
            end
            "#,
        );
        let mut host = LuaHost::new(
            params.clone(),
            PluginKind::Instrument,
            HostRole::Ui,
            44_100.0,
        );
        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        assert_eq!(params.script_macro_default(0), 4.0);
        assert!((params.script_macro_default_normalized(0) - 0.2).abs() < 1e-6);
        assert_eq!(params.script_macro_default(1), 8.0);
        assert!((params.script_macro_default_normalized(1) - 0.08).abs() < 1e-6);

        let commands = host.build_ui_frame(Vec::new()).expect("draw() failed");
        assert!(has_command(&commands, |command| matches!(
            command,
            UiCommand::Param {
                index: 1,
                scale: Some(MacroScale {
                    kind: MacroScaleType::Integer,
                    min: 1.0,
                    max: 16.0,
                }),
                ..
            }
        )));
        assert!(has_command(&commands, |command| matches!(
            command,
            UiCommand::Param {
                index: 2,
                scale: Some(MacroScale {
                    kind: MacroScaleType::Float,
                    min: 0.0,
                    max: 100.0,
                }),
                ..
            }
        )));
        assert!(has_command(&commands, |command| matches!(
            command,
            UiCommand::Label(text) if text == "Mapped count: 1"
        )));
    }

    #[test]
    fn transpose_factory_binds_octaves_to_radio_macro_and_pairs_notes() {
        let params = Arc::new(PluginParams::default());
        params.set_script(
            "transpose.lua",
            include_str!("../../../factory/midi-fx/transpose.lua"),
        );
        let mut host = LuaHost::new(params, PluginKind::MidiFx, HostRole::Ui, 44_100.0);
        assert!(host.last_error().is_none(), "{:?}", host.last_error());

        let commands = host.build_ui_frame(Vec::new()).expect("draw() failed");
        let radio = commands.iter().find_map(|command| match command {
            UiCommand::RadioParam { index, choices, .. } if *index == 1 => Some(choices),
            _ => None,
        });
        assert_eq!(
            radio,
            Some(&vec![
                "-3".to_string(),
                "-2".to_string(),
                "-1".to_string(),
                "0".to_string(),
                "+1".to_string(),
                "+2".to_string(),
                "+3".to_string(),
            ])
        );

        let midi_fn = host
            .loaded
            .as_ref()
            .and_then(|loaded| loaded.midi_fn.clone())
            .expect("transposer must define on_midi");
        midi_fn
            .call::<()>(("note_on", 60.0, 0.8, 0.0, 0.0))
            .expect("transpose note_on failed");
        midi_fn
            .call::<()>(("note_off", 60.0, 0.0, 0.0, 32.0))
            .expect("transpose note_off failed");

        let events: Vec<_> = host.midi_out.lock().drain().collect();
        assert!(matches!(
            events.as_slice(),
            [
                NoteEvent::NoteOn {
                    key: Key::Number(12),
                    voice_id: VoiceID::ID(1),
                    ..
                },
                NoteEvent::NoteOff {
                    key: Key::Number(12),
                    voice_id: VoiceID::ID(1),
                    timing: 32,
                    ..
                }
            ]
        ));
    }

    #[test]
    fn default_script_loads_and_builds_ui() {
        let params = Arc::new(PluginParams::default());
        let mut host = LuaHost::new(
            params.clone(),
            PluginKind::Instrument,
            HostRole::Ui,
            44_100.0,
        );

        assert!(
            host.last_error().is_none(),
            "default script failed to load: {:?}",
            host.last_error()
        );
        assert!(
            host.loaded
                .as_ref()
                .and_then(|loaded| loaded.process_voice_fn.as_ref())
                .is_some()
        );
        assert_eq!(params.script_macro_default(0), 0.65);
        let loaded = host.loaded.as_ref().expect("script should be loaded");
        let voice_fn = loaded
            .process_voice_fn
            .clone()
            .expect("default script should define process_voice");
        let voice = loaded.lua.create_table().expect("create voice table");
        voice.set("id", 1u64).unwrap();
        voice.set("note", 60u8).unwrap();
        voice.set("channel", 0u8).unwrap();
        voice.set("velocity", 0.8f32).unwrap();
        voice.set("gate", true).unwrap();
        voice.set("frequency", 261.6256f64).unwrap();
        voice.set("age", 1u64).unwrap();
        voice_fn
            .call::<Value>(voice)
            .expect("default process_voice failed");

        let commands = host.build_ui_frame(Vec::new()).expect("draw() failed");
        assert!(has_command(&commands, |command| matches!(
            command,
            UiCommand::Heading(_)
        )));
        assert!(has_command(&commands, |command| {
            matches!(command, UiCommand::Param { index: 1, .. })
        }));
    }

    #[test]
    fn factory_scripts_load_and_render_their_ui() {
        let examples = [
            (
                PluginKind::MidiFx,
                include_str!("../../../factory/midi-fx/16th-arpeggiator.lua"),
                0.5,
            ),
            (
                PluginKind::Instrument,
                include_str!("../../../factory/instrument/fm-donk.lua"),
                0.65,
            ),
            (
                PluginKind::Instrument,
                include_str!("../../../factory/instrument/sine-pad.lua"),
                1.0,
            ),
            (
                PluginKind::Instrument,
                include_str!("../../../factory/instrument/gated-sine-pad.lua"),
                0.65,
            ),
            (
                PluginKind::Instrument,
                include_str!("../../../factory/instrument/kick.lua"),
                0.65,
            ),
            (
                PluginKind::Instrument,
                include_str!("../../../factory/instrument/clap.lua"),
                0.7,
            ),
            (
                PluginKind::Instrument,
                include_str!("../../../factory/instrument/closed-hihat.lua"),
                0.65,
            ),
            (
                PluginKind::Instrument,
                include_str!("../../../factory/instrument/open-hihat.lua"),
                0.65,
            ),
            (
                PluginKind::Instrument,
                include_str!("../../../factory/instrument/snare.lua"),
                0.65,
            ),
            (
                PluginKind::AudioFx,
                include_str!("../../../factory/audio-fx/stereo-delay.lua"),
                0.25,
            ),
            (
                PluginKind::AudioFx,
                include_str!("../../../factory/audio-fx/stereo-compressor.lua"),
                0.625,
            ),
            (
                PluginKind::AudioFx,
                include_str!("../../../factory/audio-fx/stereo-reverb.lua"),
                0.55,
            ),
        ];

        for (kind, source, macro_1_default) in examples {
            let params = Arc::new(PluginParams::default());
            params.set_script("factory-example.lua", source);
            let mut host = LuaHost::new(params, kind, HostRole::Ui, 44_100.0);
            assert!(
                host.last_error().is_none(),
                "{kind:?}: {:?}",
                host.last_error()
            );
            assert_eq!(host.params.script_macro_default(0), macro_1_default);
            host.build_ui_frame(Vec::new())
                .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
            if source.contains("ui.separator_horizontal()") {
                let commands = host
                    .build_ui_frame(Vec::new())
                    .expect("horizontal separator draw failed");
                assert!(has_command(&commands, |command| matches!(
                    command,
                    UiCommand::HorizontalSeparator
                )));
            }
            if source.contains("ui.separator_vertical()") {
                let commands = host
                    .build_ui_frame(Vec::new())
                    .expect("vertical separator draw failed");
                assert!(has_command(&commands, |command| matches!(
                    command,
                    UiCommand::VerticalSeparator
                )));
            }

            if kind == PluginKind::Instrument {
                let loaded = host.loaded.as_ref().expect("script should be loaded");
                let voice_fn = loaded
                    .process_voice_fn
                    .clone()
                    .expect("instrument example must define process_voice");
                let voice = loaded.lua.create_table().expect("create voice table");
                voice.set("id", 1u64).unwrap();
                voice.set("note", 60u8).unwrap();
                voice.set("channel", 0u8).unwrap();
                voice.set("velocity", 0.8f32).unwrap();
                voice.set("gate", true).unwrap();
                voice.set("frequency", 261.6256f64).unwrap();
                voice.set("age", 1u64).unwrap();
                voice_fn
                    .call::<Value>(voice)
                    .unwrap_or_else(|error| panic!("{kind:?} process_voice: {error}"));
            }

            if kind == PluginKind::MidiFx {
                let midi_fn = host
                    .loaded
                    .as_ref()
                    .and_then(|loaded| loaded.midi_fn.clone())
                    .expect("arpeggiator must define on_midi");
                midi_fn
                    .call::<()>(("note_on", 60.0, 0.8, 0.0, 0.0))
                    .expect("arpeggiator note_on failed");
                midi_fn
                    .call::<()>(("note_on", 64.0, 0.7, 0.0, 0.0))
                    .expect("arpeggiator second note_on failed");
                let mut events = Vec::new();
                for _ in 0..50 {
                    host.process_midi_block(128, 44_100.0, Some(120.0), true, Some(0.0));
                    events.extend(host.midi_out.lock().drain());
                }
                assert!(
                    host.last_error().is_none(),
                    "arpeggiator: {:?}",
                    host.last_error()
                );
                assert!(events.iter().any(|event| matches!(
                    event,
                    NoteEvent::NoteOn {
                        key: Key::Number(60),
                        voice_id: VoiceID::ID(1),
                        ..
                    }
                )));
                assert!(events.iter().any(|event| matches!(
                    event,
                    NoteEvent::NoteOn {
                        key: Key::Number(64),
                        voice_id: VoiceID::ID(1),
                        ..
                    }
                )));
                assert!(events.iter().any(|event| matches!(
                    event,
                    NoteEvent::NoteOff {
                        key: Key::Number(60),
                        voice_id: VoiceID::ID(1),
                        ..
                    }
                )));
            }

            if kind == PluginKind::AudioFx {
                host.process_midi_block(0, 44_100.0, Some(120.0), false, None);
                assert!(
                    host.last_error().is_none(),
                    "{kind:?}: {:?}",
                    host.last_error()
                );
            }
        }
    }
}
