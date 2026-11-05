use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use nice_plug::prelude::*;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub const MACRO_COUNT: usize = 64;
pub const MAX_VOICES: usize = 32;
const TRACE_LEN: usize = 128;
const WAVEFORM_LEN: usize = 256;
const METER_CHANNELS: usize = 2;
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

    pub fn folder_name(self) -> &'static str {
        match self {
            Self::Instrument => "instrument",
            Self::AudioFx => "audio-fx",
            Self::MidiFx => "midi-fx",
        }
    }

    pub fn extension_folder_name(self) -> &'static str {
        match self {
            Self::Instrument => "synth",
            Self::AudioFx => "audio-fx",
            Self::MidiFx => "midi-fx",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptState {
    pub name: String,
    pub source: String,
    pub path: Option<String>,
    #[serde(default)]
    pub modules: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub extension_id: Option<String>,
    #[serde(default)]
    pub extension_version: Option<String>,
}

impl Default for ScriptState {
    fn default() -> Self {
        Self {
            name: "default.lua".to_string(),
            source: DEFAULT_SCRIPT.to_string(),
            path: None,
            modules: std::collections::BTreeMap::new(),
            extension_id: None,
            extension_version: None,
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
    pub block_size: AtomicU32,
    pub oversampling_factor: AtomicU32,
    pub editor_open: AtomicBool,
    pub debug_latency_enabled: AtomicBool,
    pub last_processing_latency_ms: AtomicU32,
    pub input_peak: AtomicU32,
    pub sidechain_peak: AtomicU32,
    level_trace: [AtomicU32; TRACE_LEN],
    level_trace_write: AtomicUsize,
    gain_trace: [AtomicU32; TRACE_LEN],
    gain_trace_write: AtomicUsize,
    waveform: [AtomicU32; WAVEFORM_LEN],
    waveform_write: AtomicUsize,
    input_channel_peaks: [AtomicU32; METER_CHANNELS],
    output_channel_peaks: [AtomicU32; METER_CHANNELS],
    sidechain_channel_peaks: [AtomicU32; METER_CHANNELS],
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
            block_size: AtomicU32::new(0),
            oversampling_factor: AtomicU32::new(1),
            editor_open: AtomicBool::new(false),
            debug_latency_enabled: AtomicBool::new(false),
            last_processing_latency_ms: AtomicU32::new(0.0f32.to_bits()),
            input_peak: AtomicU32::new(0.0f32.to_bits()),
            sidechain_peak: AtomicU32::new(0.0f32.to_bits()),
            level_trace: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            level_trace_write: AtomicUsize::new(0),
            gain_trace: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            gain_trace_write: AtomicUsize::new(0),
            waveform: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            waveform_write: AtomicUsize::new(0),
            input_channel_peaks: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            output_channel_peaks: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            sidechain_channel_peaks: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
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
            script.modules.clear();
            script.extension_id = None;
            script.extension_version = None;
        }
        self.request_reload();
    }

    pub fn set_script_file(&self, path: &std::path::Path, source: impl Into<String>) {
        {
            let mut script = self.script.lock();
            script.name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            script.source = source.into();
            script.path = Some(path.to_string_lossy().into_owned());
            script.modules.clear();
            script.extension_id = None;
            script.extension_version = None;
        }
        self.request_reload();
    }

    pub fn set_extension(
        &self,
        path: &std::path::Path,
        manifest: &crate::library::ExtensionManifest,
        source: impl Into<String>,
        modules: std::collections::BTreeMap<String, String>,
    ) {
        {
            let mut script = self.script.lock();
            script.name = manifest
                .name
                .clone()
                .or_else(|| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "extension".to_string());
            script.source = source.into();
            script.path = Some(path.to_string_lossy().into_owned());
            script.modules = modules;
            script.extension_id = Some(manifest.id.clone());
            script.extension_version = Some(manifest.version.clone());
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

    pub fn set_block_size(&self, block_size: usize) {
        self.block_size.store(block_size as u32, Ordering::Relaxed);
    }

    pub fn block_size(&self) -> usize {
        self.block_size.load(Ordering::Relaxed) as usize
    }

    pub fn editor_open(&self) -> bool {
        self.editor_open.load(Ordering::Relaxed)
    }

    pub fn set_editor_open(&self, open: bool) {
        self.editor_open.store(open, Ordering::Relaxed);
    }

    pub fn oversampling_factor(&self) -> usize {
        self.oversampling_factor
            .load(Ordering::Relaxed)
            .clamp(1, 16) as usize
    }

    pub fn set_oversampling_factor(&self, factor: usize) {
        self.oversampling_factor
            .store(factor.clamp(1, 16) as u32, Ordering::Relaxed);
    }
    pub fn debug_latency_enabled(&self) -> bool {
        self.debug_latency_enabled.load(Ordering::Relaxed)
    }

    pub fn set_debug_latency_enabled(&self, enabled: bool) {
        self.debug_latency_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn last_processing_latency_ms(&self) -> f32 {
        f32::from_bits(self.last_processing_latency_ms.load(Ordering::Relaxed))
    }

    pub fn set_last_processing_latency_ms(&self, value: f32) {
        self.last_processing_latency_ms
            .store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn set_meter_peak(&self, peak: f32) {
        self.meter_peak.store(peak.to_bits(), Ordering::Relaxed);
    }

    pub fn meter_peak(&self) -> f32 {
        f32::from_bits(self.meter_peak.load(Ordering::Relaxed))
    }

    pub fn set_input_peak(&self, peak: f32) {
        self.input_peak.store(peak.to_bits(), Ordering::Relaxed);
    }

    pub fn input_peak(&self) -> f32 {
        f32::from_bits(self.input_peak.load(Ordering::Relaxed))
    }

    pub fn set_sidechain_peak(&self, peak: f32) {
        self.sidechain_peak.store(peak.to_bits(), Ordering::Relaxed);
    }

    pub fn sidechain_peak(&self) -> f32 {
        f32::from_bits(self.sidechain_peak.load(Ordering::Relaxed))
    }

    pub fn push_level_trace(&self, value: f32) {
        let sequence = self.level_trace_write.fetch_add(1, Ordering::Relaxed);
        self.level_trace[sequence % TRACE_LEN].store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn level_trace(&self) -> Vec<f32> {
        read_trace(&self.level_trace, &self.level_trace_write)
    }

    pub fn push_gain_trace(&self, value: f32) {
        let sequence = self.gain_trace_write.fetch_add(1, Ordering::Relaxed);
        self.gain_trace[sequence % TRACE_LEN].store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn gain_trace(&self) -> Vec<f32> {
        read_trace(&self.gain_trace, &self.gain_trace_write)
    }

    pub fn push_waveform_block(&self, num_samples: usize, mut sample_at: impl FnMut(usize) -> f32) {
        let mut sequence = self.waveform_write.load(Ordering::Relaxed);
        for sample in 0..num_samples {
            self.waveform[sequence % WAVEFORM_LEN]
                .store(sample_at(sample).to_bits(), Ordering::Relaxed);
            sequence = sequence.wrapping_add(1);
        }
        self.waveform_write.store(sequence, Ordering::Release);
    }

    pub fn waveform_trace(&self) -> Vec<f32> {
        read_trace(&self.waveform, &self.waveform_write)
    }

    pub fn set_input_channel_peaks(&self, peaks: &[f32]) {
        for (slot, peak) in self.input_channel_peaks.iter().zip(peaks) {
            slot.store(peak.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn input_channel_peaks(&self) -> Vec<f32> {
        self.input_channel_peaks
            .iter()
            .map(|peak| f32::from_bits(peak.load(Ordering::Relaxed)))
            .collect()
    }

    pub fn input_channel_peak(&self, channel: usize) -> f32 {
        self.input_channel_peaks
            .get(channel)
            .map(|peak| f32::from_bits(peak.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
    }

    pub fn set_output_channel_peaks(&self, peaks: &[f32]) {
        for (slot, peak) in self.output_channel_peaks.iter().zip(peaks) {
            slot.store(peak.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn output_channel_peaks(&self) -> Vec<f32> {
        self.output_channel_peaks
            .iter()
            .map(|peak| f32::from_bits(peak.load(Ordering::Relaxed)))
            .collect()
    }

    pub fn output_channel_peak(&self, channel: usize) -> f32 {
        self.output_channel_peaks
            .get(channel)
            .map(|peak| f32::from_bits(peak.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
    }

    pub fn set_sidechain_channel_peaks(&self, peaks: &[f32]) {
        for (slot, peak) in self.sidechain_channel_peaks.iter().zip(peaks) {
            slot.store(peak.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn sidechain_channel_peaks(&self) -> Vec<f32> {
        self.sidechain_channel_peaks
            .iter()
            .map(|peak| f32::from_bits(peak.load(Ordering::Relaxed)))
            .collect()
    }

    pub fn sidechain_channel_peak(&self, channel: usize) -> f32 {
        self.sidechain_channel_peaks
            .get(channel)
            .map(|peak| f32::from_bits(peak.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
    }
}

fn read_trace<const LEN: usize>(trace: &[AtomicU32; LEN], write: &AtomicUsize) -> Vec<f32> {
    let end = write.load(Ordering::Acquire);
    let len = end.min(LEN);
    let start = end.saturating_sub(len);
    (start..end)
        .map(|sequence| f32::from_bits(trace[sequence % LEN].load(Ordering::Relaxed)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_scale_converts_integer_and_float_ranges() {
        let integer = MacroScale::new(MacroScaleType::Integer, 1.0, 16.0).unwrap();
        assert_eq!(integer.value(0.2), 4.0);
        assert!((integer.normalize(4.0) - 0.2).abs() < 1e-6);

        let float = MacroScale::new(MacroScaleType::Float, -12.0, 12.0).unwrap();
        assert_eq!(float.value(0.5), 0.0);
        assert_eq!(float.normalize(0.0), 0.5);
    }

    #[test]
    fn graph_traces_keep_only_the_latest_samples_in_order() {
        let params = PluginParams::default();
        for value in 0..(TRACE_LEN + 3) {
            params.push_level_trace(value as f32);
        }

        let trace = params.level_trace();
        assert_eq!(trace.len(), TRACE_LEN);
        assert_eq!(trace[0], 3.0);
        assert_eq!(trace[TRACE_LEN - 1], (TRACE_LEN + 2) as f32);
    }

    #[test]
    fn waveform_keeps_the_latest_block_samples_in_order() {
        let params = PluginParams::default();
        params.push_waveform_block(WAVEFORM_LEN + 3, |sample| sample as f32);

        let trace = params.waveform_trace();
        assert_eq!(trace.len(), WAVEFORM_LEN);
        assert_eq!(trace[0], 3.0);
        assert_eq!(trace[WAVEFORM_LEN - 1], (WAVEFORM_LEN + 2) as f32);
    }
}
