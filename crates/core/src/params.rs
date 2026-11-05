use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use nice_plug::prelude::*;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub const MACRO_COUNT: usize = 64;
pub const MAX_VOICES: usize = 32;
pub const DEFAULT_SCRIPT: &str = include_str!("../scripts/default.lua");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroScaleType {
    Float,
    Integer,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacroScale {
    pub kind: MacroScaleType,
    pub min: f32,
    pub max: f32,
}

impl MacroScale {
    pub fn new(kind: MacroScaleType, min: f32, max: f32) -> Option<Self> {
        if !min.is_finite()
            || !max.is_finite()
            || min >= max
            || (kind == MacroScaleType::Integer && (min.fract() != 0.0 || max.fract() != 0.0))
        {
            return None;
        }
        Some(Self { kind, min, max })
    }

    pub fn value(self, normalized: f32) -> f32 {
        let value = self.min + normalized.clamp(0.0, 1.0) * (self.max - self.min);
        match self.kind {
            MacroScaleType::Float => value,
            MacroScaleType::Integer => value.round(),
        }
    }

    pub fn normalize(self, value: f32) -> f32 {
        let value = value.clamp(self.min, self.max);
        let value = match self.kind {
            MacroScaleType::Float => value,
            MacroScaleType::Integer => value.round(),
        };
        (value - self.min) / (self.max - self.min)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum VoiceMode {
    #[id = "mono"]
    Mono,
    #[id = "poly"]
    Poly,
}

impl VoiceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mono => "mono",
            Self::Poly => "poly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    Instrument,
    AudioFx,
    MidiFx,
}

impl PluginKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Instrument => "instrument",
            Self::AudioFx => "audio_fx",
            Self::MidiFx => "midi_fx",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptState {
    pub name: String,
    pub source: String,
    pub path: Option<String>,
}

impl Default for ScriptState {
    fn default() -> Self {
        Self {
            name: "default.lua".to_string(),
            source: DEFAULT_SCRIPT.to_string(),
            path: None,
        }
    }
}

#[derive(Params)]
pub struct MacroSlot {
    #[id = "macro"]
    pub value: FloatParam,
}

impl MacroSlot {
    fn new(index: usize) -> Self {
        Self {
            value: FloatParam::new(
                format!("Macro {}", index + 1),
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(3)),
        }
    }
}

#[derive(Params)]
pub struct PluginParams {
    #[nested(array, group = "Macros")]
    pub macros: [MacroSlot; MACRO_COUNT],

    pub voice_mode: EnumParam<VoiceMode>,
    pub max_voices: IntParam,

    #[persist = "script"]
    pub script: Mutex<ScriptState>,

    pub script_version: AtomicU64,
    voice_mode_override: AtomicU32,
    max_voices_override: AtomicU32,
    script_macro_defaults: [AtomicU32; MACRO_COUNT],
    macro_scale_types: [AtomicU32; MACRO_COUNT],
    macro_scale_mins: [AtomicU32; MACRO_COUNT],
    macro_scale_maxs: [AtomicU32; MACRO_COUNT],
    pub sample_rate: AtomicU32,
    pub meter_peak: AtomicU32,
}

impl Default for PluginParams {
    fn default() -> Self {
        Self {
            macros: std::array::from_fn(MacroSlot::new),
            voice_mode: EnumParam::new("Voice Mode", VoiceMode::Poly),
            max_voices: IntParam::new(
                "Max Voices",
                8,
                IntRange::Linear {
                    min: 1,
                    max: MAX_VOICES as i32,
                },
            ),
            script: Mutex::new(ScriptState::default()),
            script_version: AtomicU64::new(0),
            voice_mode_override: AtomicU32::new(0),
            max_voices_override: AtomicU32::new(0),
            script_macro_defaults: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            macro_scale_types: std::array::from_fn(|_| AtomicU32::new(0)),
            macro_scale_mins: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            macro_scale_maxs: std::array::from_fn(|_| AtomicU32::new(1.0f32.to_bits())),
            sample_rate: AtomicU32::new(44_100.0f32.to_bits()),
            meter_peak: AtomicU32::new(0.0f32.to_bits()),
        }
    }
}

impl PluginParams {
    pub fn clear_script_macro_defaults(&self) {
        for value in &self.script_macro_defaults {
            value.store(0.0f32.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_script_macro_default(&self, index: usize, value: f32) {
        self.script_macro_defaults[index].store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn script_macro_default(&self, index: usize) -> f32 {
        f32::from_bits(self.script_macro_defaults[index].load(Ordering::Relaxed))
    }

    pub fn clear_macro_scales(&self) {
        for kind in &self.macro_scale_types {
            kind.store(0, Ordering::Release);
        }
    }

    pub fn set_macro_scale(&self, index: usize, scale: MacroScale) {
        let kind = match scale.kind {
            MacroScaleType::Float => 1,
            MacroScaleType::Integer => 2,
        };
        self.macro_scale_types[index].store(0, Ordering::Release);
        self.macro_scale_mins[index].store(scale.min.to_bits(), Ordering::Relaxed);
        self.macro_scale_maxs[index].store(scale.max.to_bits(), Ordering::Relaxed);
        self.macro_scale_types[index].store(kind, Ordering::Release);
    }

    pub fn macro_scale(&self, index: usize) -> Option<MacroScale> {
        let kind = match self.macro_scale_types[index].load(Ordering::Acquire) {
            1 => MacroScaleType::Float,
            2 => MacroScaleType::Integer,
            _ => return None,
        };
        MacroScale::new(
            kind,
            f32::from_bits(self.macro_scale_mins[index].load(Ordering::Relaxed)),
            f32::from_bits(self.macro_scale_maxs[index].load(Ordering::Relaxed)),
        )
    }

    pub fn macro_value(&self, index: usize) -> f32 {
        let value = self.macros[index].value.value();
        self.macro_scale(index)
            .map(|scale| scale.value(value))
            .unwrap_or(value)
    }

    pub fn script_macro_default_normalized(&self, index: usize) -> f32 {
        let value = self.script_macro_default(index);
        self.macro_scale(index)
            .map(|scale| scale.normalize(value))
            .unwrap_or_else(|| value.clamp(0.0, 1.0))
    }

    pub fn effective_voice_mode(&self) -> VoiceMode {
        match self.voice_mode_override.load(Ordering::Relaxed) {
            1 => VoiceMode::Mono,
            2 => VoiceMode::Poly,
            _ => self.voice_mode.value(),
        }
    }

    pub fn set_voice_mode_override(&self, mode: Option<VoiceMode>) {
        self.voice_mode_override.store(
            match mode {
                Some(VoiceMode::Mono) => 1,
                Some(VoiceMode::Poly) => 2,
                None => 0,
            },
            Ordering::Relaxed,
        );
    }

    pub fn effective_max_voices(&self) -> usize {
        let overridden = self.max_voices_override.load(Ordering::Relaxed) as usize;
        if overridden == 0 {
            self.max_voices.value().clamp(1, MAX_VOICES as i32) as usize
        } else {
            overridden.clamp(1, MAX_VOICES)
        }
    }

    pub fn set_max_voices_override(&self, max_voices: Option<usize>) {
        self.max_voices_override.store(
            max_voices
                .map(|voices| voices.clamp(1, MAX_VOICES) as u32)
                .unwrap_or(0),
            Ordering::Relaxed,
        );
    }

    pub fn request_reload(&self) {
        self.script_version.fetch_add(1, Ordering::AcqRel);
    }

    pub fn set_script(&self, name: impl Into<String>, source: impl Into<String>) {
        {
            let mut script = self.script.lock();
            script.name = name.into();
            script.source = source.into();
            script.path = None;
        }
        self.request_reload();
    }

    pub fn set_sample_rate(&self, sample_rate: f32) {
        self.sample_rate
            .store(sample_rate.to_bits(), Ordering::Relaxed);
    }

    pub fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }

    pub fn set_meter_peak(&self, peak: f32) {
        self.meter_peak.store(peak.to_bits(), Ordering::Relaxed);
    }

    pub fn meter_peak(&self) -> f32 {
        f32::from_bits(self.meter_peak.load(Ordering::Relaxed))
    }
}
