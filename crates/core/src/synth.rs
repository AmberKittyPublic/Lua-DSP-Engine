use std::f64::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use mlua::{AnyUserData, Lua, Table, UserData, UserDataMethods};

use crate::audio::AudioBridge;
use crate::params::{MAX_VOICES, PluginParams, VoiceMode};

const MAX_UNISON: usize = 16;
static NEXT_NOISE_SEED: AtomicU32 = AtomicU32::new(0x1234_5678);

#[derive(Clone, Copy)]
enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
    WhiteNoise,
    PinkNoise,
    BrownNoise,
}

impl Waveform {
    fn parse(value: &str) -> mlua::Result<Self> {
        match value {
            "sine" | "sin" => Ok(Self::Sine),
            "saw" => Ok(Self::Saw),
            "square" => Ok(Self::Square),
            "triangle" => Ok(Self::Triangle),
            "noise" | "white" | "white_noise" => Ok(Self::WhiteNoise),
            "pink" | "pink_noise" => Ok(Self::PinkNoise),
            "brown" | "brown_noise" => Ok(Self::BrownNoise),
            _ => Err(mlua::Error::RuntimeError(format!(
                "unknown oscillator waveform: {value}"
            ))),
        }
    }

    fn parse_lfo(value: &str) -> mlua::Result<Self> {
        let waveform = Self::parse(value)?;
        if matches!(
            waveform,
            Self::WhiteNoise | Self::PinkNoise | Self::BrownNoise
        ) {
            return Err(mlua::Error::RuntimeError(
                "noise is an oscillator waveform, not an LFO waveform".to_string(),
            ));
        }
        Ok(waveform)
    }

    fn sample(self, phase: f64) -> f64 {
        match self {
            Self::Sine => (phase * TAU).sin(),
            Self::Saw => phase * 2.0 - 1.0,
            Self::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Self::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            Self::WhiteNoise | Self::PinkNoise | Self::BrownNoise => 0.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DistortionMode {
    Saturator,
    Bitcrush,
    Tube,
    Tape,
    Overdrive,
    Fuzz,
    Diode,
}

impl DistortionMode {
    fn parse(value: &str) -> mlua::Result<Self> {
        match value {
            "saturator" => Ok(Self::Saturator),
            "bitcrush" | "bitcrush_downsample" => Ok(Self::Bitcrush),
            "tube" => Ok(Self::Tube),
            "tape" => Ok(Self::Tape),
            "overdrive" => Ok(Self::Overdrive),
            "fuzz" => Ok(Self::Fuzz),
            "diode" => Ok(Self::Diode),
            _ => Err(mlua::Error::RuntimeError(format!(
                "unknown oscillator distortion mode: {value}"
            ))),
        }
    }
}

fn distort_sample(mode: DistortionMode, value: f64, bias: f64, character: f64, bits: u32) -> f64 {
    match mode {
        DistortionMode::Saturator => ((value + bias * 0.15) * (1.0 + character * 4.0)).tanh(),
        DistortionMode::Bitcrush => {
            let levels = 2.0_f64.powi(bits as i32 - 1);
            (value.clamp(-1.0, 1.0) * levels + 0.5).floor() / levels
        }
        DistortionMode::Tube => {
            let asymmetric = value + bias * 0.2;
            if asymmetric >= 0.0 {
                (asymmetric * (1.2 + character * 2.0)).tanh()
            } else {
                (asymmetric * (0.75 + character * 1.25)).tanh()
            }
        }
        DistortionMode::Tape => {
            let soft = ((value + bias * 0.1) * 1.35).tanh();
            let compressed = value.clamp(-1.0, 1.0);
            soft * (1.0 - character * 0.25) + compressed * character * 0.25
        }
        DistortionMode::Overdrive => {
            let threshold = 1.0 - character * 0.9;
            value.clamp(-threshold, threshold) / threshold
        }
        DistortionMode::Fuzz => {
            let fuzz = value * (2.0 + character * 12.0) + bias * 0.2;
            if fuzz.abs() < 0.015 { 0.0 } else { fuzz.tanh() }
        }
        DistortionMode::Diode => {
            let diode = value + bias * 0.15;
            let excess = diode.abs() - (0.05 + character * 0.35);
            if excess <= 0.0 {
                0.0
            } else {
                let polarity = if diode < 0.0 { -1.0 } else { 1.0 };
                polarity * (1.0 - (-excess * (3.0 + character * 15.0)).exp())
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_distortion(
    input: f64,
    channel: usize,
    mode: DistortionMode,
    drive: f64,
    mix: f64,
    bias: f64,
    character: f64,
    bits: u32,
    downsample: usize,
    held: &mut [f64; 3],
    counter: &mut [usize; 3],
) -> f64 {
    let driven = input * drive;
    let wet = if mode == DistortionMode::Bitcrush {
        if counter[channel] == 0 {
            held[channel] = distort_sample(mode, driven, bias, character, bits);
        }
        counter[channel] = (counter[channel] + 1) % downsample;
        held[channel]
    } else {
        distort_sample(mode, driven, bias, character, bits)
    };
    input + (wet - input) * mix
}

fn next_noise(state: &mut u32) -> f64 {
    let mut value = *state;
    value ^= value << 13;
    value ^= value >> 17;
    value ^= value << 5;
    *state = value;
    value as f64 / u32::MAX as f64 * 2.0 - 1.0
}

struct NoiseState {
    white_seed: u32,
    pink_state: [f64; 7],
    brown_state: f64,
}

impl NoiseState {
    fn new(seed: u32) -> Self {
        Self {
            white_seed: seed.max(1),
            pink_state: [0.0; 7],
            brown_state: 0.0,
        }
    }

    fn sample(&mut self, waveform: Waveform) -> f64 {
        let white = next_noise(&mut self.white_seed);
        match waveform {
            Waveform::WhiteNoise => white,
            Waveform::PinkNoise => {
                self.pink_state[0] = 0.99886 * self.pink_state[0] + white * 0.0555179;
                self.pink_state[1] = 0.99332 * self.pink_state[1] + white * 0.0750759;
                self.pink_state[2] = 0.96900 * self.pink_state[2] + white * 0.1538520;
                self.pink_state[3] = 0.86650 * self.pink_state[3] + white * 0.3104856;
                self.pink_state[4] = 0.55000 * self.pink_state[4] + white * 0.5329522;
                self.pink_state[5] = -0.7616 * self.pink_state[5] - white * 0.0168980;
                let pink =
                    self.pink_state[..6].iter().sum::<f64>() + self.pink_state[6] + white * 0.5362;
                self.pink_state[6] = white * 0.115926;
                pink * 0.11
            }
            Waveform::BrownNoise => {
                self.brown_state = (self.brown_state + white * 0.02) / 1.02;
                self.brown_state * 3.5
            }
            _ => 0.0,
        }
    }
}

struct Lfo {
    waveform: Waveform,
    rate: f64,
    sync_beats: Option<f64>,
    tempo: f64,
    phase: f64,
    inverted: bool,
}

impl Lfo {
    fn rate_hz(&self) -> f64 {
        self.sync_beats
            .map(|beats| self.tempo / 60.0 / beats)
            .unwrap_or(self.rate)
    }

    fn next_sample(&mut self, sample_rate: f64) -> f64 {
        let value = self.value();
        if sample_rate.is_finite() && sample_rate > 0.0 {
            self.phase = (self.phase + self.rate_hz() / sample_rate).rem_euclid(1.0);
        }
        value
    }

    fn value(&self) -> f64 {
        let value = self.waveform.sample(self.phase);
        if self.inverted { -value } else { value }
    }

    fn process(&mut self, num_samples: usize, sample_rate: f64) -> f64 {
        if !sample_rate.is_finite() || sample_rate <= 0.0 || num_samples == 0 {
            return self.value();
        }

        let mut sum = 0.0;
        for _ in 0..num_samples {
            sum += self.next_sample(sample_rate);
        }
        sum / num_samples as f64
    }
}

impl UserData for Lfo {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("set_rate", |_, this, rate: f64| {
            this.rate = rate.clamp(0.0, 20_000.0);
            this.sync_beats = None;
            Ok(())
        });
        methods.add_method_mut("set_sync", |_, this, division: String| {
            this.sync_beats = Some(parse_sync_division(&division)?);
            Ok(())
        });
        methods.add_method_mut("set_tempo", |_, this, tempo: f64| {
            if !tempo.is_finite() || tempo <= 0.0 {
                return Err(mlua::Error::RuntimeError(
                    "LFO tempo must be a finite positive BPM value".to_string(),
                ));
            }
            this.tempo = tempo.clamp(1.0, 1000.0);
            Ok(())
        });
        methods.add_method("rate", |_, this, ()| Ok(this.rate_hz()));
        methods.add_method_mut("reset", |_, this, phase: Option<f64>| {
            this.phase = phase.unwrap_or(0.0).rem_euclid(1.0);
            Ok(())
        });
        methods.add_method("value", |_, this, ()| Ok(this.value()));
        methods.add_method_mut("set_inverted", |_, this, inverted: bool| {
            this.inverted = inverted;
            Ok(())
        });
        methods.add_method_mut(
            "process",
            |_, this, (num_samples, sample_rate): (usize, f64)| {
                Ok(this.process(num_samples, sample_rate))
            },
        );
    }
}

fn parse_sync_division(division: &str) -> mlua::Result<f64> {
    let division = division.trim().to_ascii_lowercase();
    let (note, modifier) = match division.chars().last() {
        Some('d' | 't') => division.split_at(division.len() - 1),
        _ => (division.as_str(), ""),
    };
    let denominator = note
        .strip_prefix("1/")
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| matches!(value, 1 | 2 | 4 | 8 | 16 | 32 | 64));
    let Some(denominator) = denominator else {
        return Err(mlua::Error::RuntimeError(format!(
            "unsupported LFO sync division: {division}"
        )));
    };

    let beats = 4.0 / denominator as f64;
    Ok(match modifier {
        "d" => beats * 1.5,
        "t" => beats * (2.0 / 3.0),
        _ => beats,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EnvelopeStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

struct Envelope {
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    level: f64,
    gate: bool,
    stage: EnvelopeStage,
    target: f64,
    duration: f64,
    step: f64,
    remaining: usize,
    needs_init: bool,
}

impl Envelope {
    fn new(attack: f64, decay: f64, sustain: f64, release: f64) -> Self {
        Self {
            attack: attack.max(0.0),
            decay: decay.max(0.0),
            sustain: sustain.clamp(0.0, 1.0),
            release: release.max(0.0),
            level: 0.0,
            gate: false,
            stage: EnvelopeStage::Idle,
            target: 0.0,
            duration: 0.0,
            step: 0.0,
            remaining: 0,
            needs_init: false,
        }
    }

    fn trigger(&mut self) {
        self.gate = true;
        self.level = 0.0;
        self.enter_stage(EnvelopeStage::Attack, 1.0, self.attack);
    }

    fn set_gate(&mut self, gate: bool) {
        if gate == self.gate {
            return;
        }

        self.gate = gate;
        if gate {
            self.enter_stage(EnvelopeStage::Attack, 1.0, self.attack);
        } else {
            self.enter_stage(EnvelopeStage::Release, 0.0, self.release);
        }
    }

    fn set_adsr(&mut self, attack: f64, decay: f64, sustain: f64, release: f64) {
        let attack = attack.clamp(0.0, 60.0);
        let decay = decay.clamp(0.0, 60.0);
        let sustain = sustain.clamp(0.0, 1.0);
        let release = release.clamp(0.0, 60.0);
        let attack_changed = self.attack != attack;
        let decay_changed = self.decay != decay;
        let sustain_changed = self.sustain != sustain;
        let release_changed = self.release != release;

        self.attack = attack;
        self.decay = decay;
        self.sustain = sustain;
        self.release = release;

        match self.stage {
            EnvelopeStage::Attack if attack_changed => {
                self.enter_stage(EnvelopeStage::Attack, 1.0, attack);
            }
            EnvelopeStage::Decay if decay_changed || sustain_changed => {
                self.enter_stage(EnvelopeStage::Decay, sustain, decay);
            }
            EnvelopeStage::Sustain if sustain_changed => {
                self.level = sustain;
                self.target = sustain;
            }
            EnvelopeStage::Release if release_changed => {
                self.enter_stage(EnvelopeStage::Release, 0.0, release);
            }
            _ => {}
        }
    }

    fn process(&mut self, num_samples: usize, sample_rate: f64) -> f64 {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return self.level;
        }

        let mut processed = 0;
        while processed < num_samples {
            if self.stage == EnvelopeStage::Idle || self.stage == EnvelopeStage::Sustain {
                break;
            }

            if self.needs_init {
                self.initialize_stage(sample_rate);
                if self.needs_init {
                    continue;
                }
                if self.remaining == 0 {
                    self.complete_stage();
                    continue;
                }
            }

            self.level += self.step;
            self.remaining -= 1;
            processed += 1;
            if self.remaining == 0 {
                self.complete_stage();
            }
        }

        self.level
    }

    fn enter_stage(&mut self, stage: EnvelopeStage, target: f64, duration: f64) {
        self.stage = stage;
        self.target = target;
        self.duration = duration;
        self.remaining = 0;
        self.needs_init = true;
    }

    fn initialize_stage(&mut self, sample_rate: f64) {
        self.remaining = (self.duration * sample_rate).round() as usize;
        self.step = if self.remaining == 0 {
            0.0
        } else {
            (self.target - self.level) / self.remaining as f64
        };
        self.needs_init = false;
    }

    fn complete_stage(&mut self) {
        self.level = self.target;
        match self.stage {
            EnvelopeStage::Attack => {
                self.enter_stage(EnvelopeStage::Decay, self.sustain, self.decay);
            }
            EnvelopeStage::Decay => {
                self.stage = EnvelopeStage::Sustain;
                self.needs_init = false;
            }
            EnvelopeStage::Release => {
                self.stage = EnvelopeStage::Idle;
                self.needs_init = false;
            }
            EnvelopeStage::Idle | EnvelopeStage::Sustain => {}
        }
    }
}

impl UserData for Envelope {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("gate", |_, this, gate: bool| {
            this.set_gate(gate);
            Ok(())
        });
        methods.add_method_mut("trigger", |_, this, ()| {
            this.trigger();
            Ok(())
        });
        methods.add_method_mut(
            "set_adsr",
            |_, this, (attack, decay, sustain, release): (f64, f64, f64, f64)| {
                if !attack.is_finite()
                    || !decay.is_finite()
                    || !sustain.is_finite()
                    || !release.is_finite()
                {
                    return Err(mlua::Error::RuntimeError(
                        "ADSR values must be finite numbers".to_string(),
                    ));
                }
                this.set_adsr(attack, decay, sustain, release);
                Ok(())
            },
        );
        methods.add_method("value", |_, this, ()| Ok(this.level));
        methods.add_method("done", |_, this, ()| Ok(this.stage == EnvelopeStage::Idle));
        methods.add_method_mut(
            "process",
            |_, this, (num_samples, sample_rate): (usize, f64)| {
                Ok(this.process(num_samples, sample_rate))
            },
        );
    }
}

#[derive(Clone, Copy, Default)]
struct FilterState {
    integrator_1: f64,
    integrator_2: f64,
}

impl FilterState {
    fn outputs(
        &mut self,
        input: f64,
        cutoff: f64,
        sample_rate: f64,
        resonance: f64,
    ) -> (f64, f64, f64) {
        let cutoff = cutoff.clamp(5.0, sample_rate * 0.45);
        let q = 0.5 + resonance * 9.5;
        let g = (std::f64::consts::PI * cutoff / sample_rate).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = input - self.integrator_2;
        let v1 = a1 * self.integrator_1 + a2 * v3;
        let v2 = self.integrator_2 + a2 * self.integrator_1 + a3 * v3;
        let high = input - (1.0 / q) * v1 - v2;
        self.integrator_1 = 2.0 * v1 - self.integrator_1;
        self.integrator_2 = 2.0 * v2 - self.integrator_2;
        (v2, v1, high)
    }
}

#[derive(Clone, Copy)]
enum FilterMode {
    Lowpass,
    Highpass,
}

#[derive(Clone, Copy)]
struct FilterStage {
    cutoff: f64,
    resonance: f64,
    enabled: bool,
    mode: FilterMode,
    states: [FilterState; 3],
}

impl Default for FilterStage {
    fn default() -> Self {
        Self {
            cutoff: 20_000.0,
            resonance: 0.0,
            enabled: false,
            mode: FilterMode::Lowpass,
            states: [FilterState::default(); 3],
        }
    }
}

impl FilterStage {
    fn configure(&mut self, cutoff: f64, resonance: f64, mode: FilterMode) {
        self.cutoff = cutoff;
        self.resonance = resonance;
        self.enabled = true;
        self.mode = mode;
    }

    fn process(&mut self, channel: usize, input: f64, cutoff: f64, sample_rate: f64) -> f64 {
        let (low, _, high) =
            self.states[channel].outputs(input, cutoff, sample_rate, self.resonance);
        match self.mode {
            FilterMode::Lowpass => low,
            FilterMode::Highpass => high,
        }
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

struct RenderModulation<'a> {
    envelope: Option<&'a mut Envelope>,
    lfo: Option<&'a mut Lfo>,
    envelope_depth_octaves: f64,
    lfo_depth_octaves: f64,
}

impl Default for RenderModulation<'_> {
    fn default() -> Self {
        Self {
            envelope: None,
            lfo: None,
            envelope_depth_octaves: 0.0,
            lfo_depth_octaves: 0.0,
        }
    }
}

type LuaFilteredRenderArgs = (
    f64,
    f64,
    f64,
    Option<AnyUserData>,
    Option<AnyUserData>,
    Option<f64>,
    Option<f64>,
);

struct Oscillator {
    audio: Arc<AudioBridge>,
    waveform: Waveform,
    phases: Vec<f64>,
    detune_cents: f64,
    stereo_spread: f64,
    fm_ratio: f64,
    fm_index: f64,
    pd_amount: f64,
    rm_ratio: f64,
    rm_depth: f64,
    pitch_drop_semitones: f64,
    pitch_drop_decay_seconds: f64,
    pitch_drop_level: f64,
    distortion_mode: DistortionMode,
    distortion_drive_db: f64,
    distortion_envelope_depth_db: f64,
    distortion_mix: f64,
    distortion_bias: f64,
    distortion_character: f64,
    distortion_bits: u32,
    distortion_downsample: usize,
    distortion_hold: [f64; 3],
    distortion_counter: [usize; 3],
    noise_state: NoiseState,
    filters: [FilterStage; 2],
}

impl Oscillator {
    fn new(audio: Arc<AudioBridge>, waveform: Waveform) -> Self {
        Self {
            audio,
            waveform,
            phases: vec![0.0],
            detune_cents: 0.0,
            stereo_spread: 0.0,
            fm_ratio: 2.0,
            fm_index: 0.0,
            pd_amount: 0.0,
            rm_ratio: 1.0,
            rm_depth: 0.0,
            pitch_drop_semitones: 0.0,
            pitch_drop_decay_seconds: 0.03,
            pitch_drop_level: 0.0,
            distortion_mode: DistortionMode::Saturator,
            distortion_drive_db: 0.0,
            distortion_envelope_depth_db: 0.0,
            distortion_mix: 0.0,
            distortion_bias: 0.0,
            distortion_character: 0.35,
            distortion_bits: 8,
            distortion_downsample: 1,
            distortion_hold: [0.0; 3],
            distortion_counter: [0; 3],
            noise_state: NoiseState::new(NEXT_NOISE_SEED.fetch_add(0x9e37_79b9, Ordering::Relaxed)),
            filters: [FilterStage::default(); 2],
        }
    }

    fn set_lowpass(&mut self, cutoff: f64, resonance: f64) {
        self.filters[0].configure(cutoff, resonance, FilterMode::Lowpass);
    }

    fn set_lowpass_2(&mut self, cutoff: f64, resonance: f64) {
        self.filters[1].configure(cutoff, resonance, FilterMode::Lowpass);
    }

    fn set_highpass(&mut self, cutoff: f64, resonance: f64) {
        self.filters[0].configure(cutoff, resonance, FilterMode::Highpass);
    }

    fn set_highpass_2(&mut self, cutoff: f64, resonance: f64) {
        self.filters[1].configure(cutoff, resonance, FilterMode::Highpass);
    }

    fn clear_filter(&mut self) {
        for filter in &mut self.filters {
            filter.clear();
        }
    }

    fn set_pitch_drop(&mut self, semitones: f64, decay_seconds: f64) {
        self.pitch_drop_semitones = semitones.clamp(0.0, 48.0);
        self.pitch_drop_decay_seconds = decay_seconds.clamp(0.001, 5.0);
        self.pitch_drop_level = if self.pitch_drop_semitones > 0.0 {
            1.0
        } else {
            0.0
        };
    }

    #[allow(clippy::too_many_arguments)]
    fn set_distortion(
        &mut self,
        mode: &str,
        drive_db: f64,
        envelope_depth_db: f64,
        mix: f64,
        bias: f64,
        character: f64,
        bits: u32,
        downsample: usize,
    ) -> mlua::Result<()> {
        if ![drive_db, envelope_depth_db, mix, bias, character]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(mlua::Error::RuntimeError(
                "distortion controls must be finite".to_string(),
            ));
        }

        self.distortion_mode = DistortionMode::parse(mode)?;
        self.distortion_drive_db = drive_db.clamp(0.0, 36.0);
        self.distortion_envelope_depth_db = envelope_depth_db.clamp(0.0, 36.0);
        self.distortion_mix = mix.clamp(0.0, 1.0);
        self.distortion_bias = bias.clamp(-1.0, 1.0);
        self.distortion_character = character.clamp(0.0, 1.0);
        self.distortion_bits = bits.clamp(1, 16);
        self.distortion_downsample = downsample.clamp(1, 32);
        Ok(())
    }

    fn render(&mut self, frequency: f64, sample_rate: f64, gain: f64) {
        self.render_inner(frequency, sample_rate, gain, RenderModulation::default());
    }

    fn render_env(&mut self, frequency: f64, sample_rate: f64, gain: f64, envelope: &mut Envelope) {
        self.render_inner(
            frequency,
            sample_rate,
            gain,
            RenderModulation {
                envelope: Some(envelope),
                ..RenderModulation::default()
            },
        );
    }

    fn render_filtered(
        &mut self,
        frequency: f64,
        sample_rate: f64,
        gain: f64,
        modulation: RenderModulation<'_>,
    ) {
        self.render_inner(frequency, sample_rate, gain, modulation);
    }

    fn render_inner(
        &mut self,
        frequency: f64,
        sample_rate: f64,
        gain: f64,
        mut modulation: RenderModulation<'_>,
    ) {
        if !frequency.is_finite() || !sample_rate.is_finite() || sample_rate <= 0.0 {
            return;
        }

        let audio = self.audio.clone();
        let waveform = self.waveform;
        let detune_cents = self.detune_cents;
        let stereo_spread = self.stereo_spread;
        let fm_ratio = self.fm_ratio;
        let fm_index = self.fm_index;
        let pd_amount = self.pd_amount;
        let rm_ratio = self.rm_ratio;
        let rm_depth = self.rm_depth;
        let pitch_drop_semitones = self.pitch_drop_semitones;
        let pitch_drop_decay_seconds = self.pitch_drop_decay_seconds;
        let mut pitch_drop_level = self.pitch_drop_level;
        let distortion_mode = self.distortion_mode;
        let distortion_base_drive = 10.0_f64.powf(self.distortion_drive_db / 20.0);
        let distortion_envelope_drive = 10.0_f64.powf(self.distortion_envelope_depth_db / 20.0);
        let distortion_mix = self.distortion_mix;
        let distortion_bias = self.distortion_bias;
        let distortion_character = self.distortion_character;
        let distortion_bits = self.distortion_bits;
        let distortion_downsample = self.distortion_downsample;
        let distortion_hold = &mut self.distortion_hold;
        let distortion_counter = &mut self.distortion_counter;
        let filters = &mut self.filters;
        let phases = &mut self.phases;
        let noise_state = &mut self.noise_state;
        let pitch_drop_coefficient = (-1.0 / (pitch_drop_decay_seconds * sample_rate)).exp();
        let has_envelope = modulation.envelope.is_some();
        audio.with_access(|access| {
            let count = phases.len();
            let normalization = 1.0 / count as f64;
            for sample_index in 0..access.num_samples() {
                let envelope_level = modulation
                    .envelope
                    .as_deref_mut()
                    .map(|envelope| envelope.process(1, sample_rate))
                    .unwrap_or(0.0);
                let lfo_value = modulation
                    .lfo
                    .as_deref_mut()
                    .map(|lfo| lfo.next_sample(sample_rate))
                    .unwrap_or(0.0);
                let sample_gain = gain * if has_envelope { envelope_level } else { 1.0 };
                let pitch_ratio = 2.0_f64.powf(pitch_drop_semitones * pitch_drop_level / 12.0);
                let step = frequency * pitch_ratio / sample_rate;
                let mut left = 0.0;
                let mut right = 0.0;
                let mut mono = 0.0;

                for (index, phase) in phases.iter_mut().enumerate() {
                    let position = if count == 1 {
                        0.0
                    } else {
                        index as f64 / (count - 1) as f64 * 2.0 - 1.0
                    };
                    let detune = 2.0_f64.powf(position * detune_cents / 1200.0);
                    let carrier_phase = phase.rem_euclid(1.0);
                    let modulator = (carrier_phase * fm_ratio * TAU).sin();
                    let mut phase_warped =
                        (carrier_phase + fm_index / TAU * modulator).rem_euclid(1.0);
                    phase_warped = (phase_warped + pd_amount * (phase_warped * TAU).sin() / TAU)
                        .rem_euclid(1.0);

                    let carrier = match waveform {
                        Waveform::WhiteNoise | Waveform::PinkNoise | Waveform::BrownNoise => {
                            noise_state.sample(waveform)
                        }
                        _ => waveform.sample(phase_warped),
                    };
                    let ring = (carrier_phase * rm_ratio * TAU).sin();
                    let value = carrier * ((1.0 - rm_depth) + rm_depth * ring);
                    mono += value;

                    let pan = (position * stereo_spread).clamp(-1.0, 1.0);
                    let angle = (pan + 1.0) * std::f64::consts::FRAC_PI_4;
                    left += value * angle.cos();
                    right += value * angle.sin();

                    *phase = (carrier_phase + step * detune).rem_euclid(1.0);
                }
                pitch_drop_level *= pitch_drop_coefficient;

                let channels = access.num_channels();
                for (index, filter) in filters.iter_mut().enumerate() {
                    if !filter.enabled {
                        continue;
                    }

                    let octaves = if index == 0 {
                        modulation.lfo_depth_octaves * lfo_value
                    } else {
                        modulation.envelope_depth_octaves * envelope_level
                    };
                    let cutoff = filter.cutoff * 2.0_f64.powf(octaves);
                    if channels == 1 {
                        mono = filter.process(2, mono, cutoff, sample_rate);
                    } else if channels >= 2 {
                        left = filter.process(0, left, cutoff, sample_rate);
                        right = filter.process(1, right, cutoff, sample_rate);
                        if channels > 2 {
                            mono = filter.process(2, mono, cutoff, sample_rate);
                        }
                    }
                }

                if distortion_mix > 0.0 {
                    let drive =
                        distortion_base_drive * distortion_envelope_drive.powf(envelope_level);
                    if channels == 1 {
                        mono = apply_distortion(
                            mono,
                            0,
                            distortion_mode,
                            drive,
                            distortion_mix,
                            distortion_bias,
                            distortion_character,
                            distortion_bits,
                            distortion_downsample,
                            distortion_hold,
                            distortion_counter,
                        );
                    } else if channels >= 2 {
                        left = apply_distortion(
                            left,
                            0,
                            distortion_mode,
                            drive,
                            distortion_mix,
                            distortion_bias,
                            distortion_character,
                            distortion_bits,
                            distortion_downsample,
                            distortion_hold,
                            distortion_counter,
                        );
                        right = apply_distortion(
                            right,
                            1,
                            distortion_mode,
                            drive,
                            distortion_mix,
                            distortion_bias,
                            distortion_character,
                            distortion_bits,
                            distortion_downsample,
                            distortion_hold,
                            distortion_counter,
                        );
                        if channels > 2 {
                            mono = apply_distortion(
                                mono,
                                2,
                                distortion_mode,
                                drive,
                                distortion_mix,
                                distortion_bias,
                                distortion_character,
                                distortion_bits,
                                distortion_downsample,
                                distortion_hold,
                                distortion_counter,
                            );
                        }
                    }
                }

                if channels == 1 {
                    access.add(0, sample_index, (mono * normalization * sample_gain) as f32);
                } else if channels >= 2 {
                    access.add(0, sample_index, (left * normalization * sample_gain) as f32);
                    access.add(
                        1,
                        sample_index,
                        (right * normalization * sample_gain) as f32,
                    );
                    for channel in 2..channels {
                        access.add(
                            channel,
                            sample_index,
                            (mono * normalization * sample_gain) as f32,
                        );
                    }
                }
            }
        });
    }
}

impl UserData for Oscillator {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("set_wave", |_, this, waveform: String| {
            this.waveform = Waveform::parse(&waveform)?;
            Ok(())
        });

        methods.add_method_mut(
            "set_unison",
            |_, this, (voices, detune_cents, spread): (usize, Option<f64>, Option<f64>)| {
                let voices = voices.clamp(1, MAX_UNISON);
                this.phases.resize(voices, 0.0);
                this.detune_cents = detune_cents.unwrap_or(0.0).clamp(-100.0, 100.0);
                this.stereo_spread = spread.unwrap_or(0.0).clamp(0.0, 1.0);
                Ok(())
            },
        );

        methods.add_method_mut("set_fm", |_, this, (ratio, index): (f64, f64)| {
            this.fm_ratio = ratio.clamp(0.0, 32.0);
            this.fm_index = index.clamp(0.0, 32.0);
            Ok(())
        });

        methods.add_method_mut("set_pd", |_, this, amount: f64| {
            this.pd_amount = amount.clamp(-0.95, 0.95);
            Ok(())
        });

        methods.add_method_mut("set_rm", |_, this, (ratio, depth): (f64, f64)| {
            this.rm_ratio = ratio.clamp(0.0, 32.0);
            this.rm_depth = depth.clamp(0.0, 1.0);
            Ok(())
        });

        methods.add_method_mut("set_lowpass", |_, this, (cutoff, resonance): (f64, f64)| {
            if !cutoff.is_finite() || cutoff <= 0.0 || !resonance.is_finite() {
                return Err(mlua::Error::RuntimeError(
                    "low-pass cutoff and resonance must be finite, with cutoff above zero"
                        .to_string(),
                ));
            }
            this.set_lowpass(cutoff, resonance.clamp(0.0, 1.0));
            Ok(())
        });

        methods.add_method_mut(
            "set_lowpass_2",
            |_, this, (cutoff, resonance): (f64, f64)| {
                if !cutoff.is_finite() || cutoff <= 0.0 || !resonance.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "low-pass cutoff and resonance must be finite, with cutoff above zero"
                            .to_string(),
                    ));
                }
                this.set_lowpass_2(cutoff, resonance.clamp(0.0, 1.0));
                Ok(())
            },
        );

        methods.add_method_mut(
            "set_highpass",
            |_, this, (cutoff, resonance): (f64, f64)| {
                if !cutoff.is_finite() || cutoff <= 0.0 || !resonance.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "high-pass cutoff and resonance must be finite, with cutoff above zero"
                            .to_string(),
                    ));
                }
                this.set_highpass(cutoff, resonance.clamp(0.0, 1.0));
                Ok(())
            },
        );

        methods.add_method_mut(
            "set_highpass_2",
            |_, this, (cutoff, resonance): (f64, f64)| {
                if !cutoff.is_finite() || cutoff <= 0.0 || !resonance.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "high-pass cutoff and resonance must be finite, with cutoff above zero"
                            .to_string(),
                    ));
                }
                this.set_highpass_2(cutoff, resonance.clamp(0.0, 1.0));
                Ok(())
            },
        );

        methods.add_method_mut("clear_filter", |_, this, ()| {
            this.clear_filter();
            Ok(())
        });

        methods.add_method_mut(
            "set_distortion",
            |_,
             this,
             (mode, drive_db, envelope_depth_db, mix, bias, character, bits, downsample): (
                String,
                f64,
                f64,
                f64,
                f64,
                f64,
                u32,
                usize,
            )| {
                this.set_distortion(
                    &mode,
                    drive_db,
                    envelope_depth_db,
                    mix,
                    bias,
                    character,
                    bits,
                    downsample,
                )
            },
        );

        methods.add_method_mut(
            "set_pitch_drop",
            |_, this, (semitones, decay_seconds): (f64, f64)| {
                if !semitones.is_finite() || !decay_seconds.is_finite() || decay_seconds <= 0.0 {
                    return Err(mlua::Error::RuntimeError(
                        "pitch-drop semitones and decay must be finite, and decay must be positive"
                            .to_string(),
                    ));
                }
                this.set_pitch_drop(semitones, decay_seconds);
                Ok(())
            },
        );

        methods.add_method_mut(
            "render",
            |_, this, (frequency, sample_rate, gain): (f64, f64, Option<f64>)| {
                this.render(frequency, sample_rate, gain.unwrap_or(1.0));
                Ok(())
            },
        );

        methods.add_method_mut(
            "render_env",
            |_, this, (frequency, sample_rate, gain, envelope): (
                f64,
                f64,
                Option<f64>,
                AnyUserData,
            )| {
                let mut envelope = envelope.borrow_mut::<Envelope>()?;
                this.render_env(
                    frequency,
                    sample_rate,
                    gain.unwrap_or(1.0),
                    &mut envelope,
                );
                Ok(())
            },
        );

        methods.add_method_mut(
            "render_filtered",
            |_, this, (frequency, sample_rate, gain, envelope, lfo, envelope_depth, lfo_depth): LuaFilteredRenderArgs| {
                let mut envelope = envelope
                    .map(|envelope| envelope.borrow_mut::<Envelope>())
                    .transpose()?;
                let mut lfo = lfo.map(|lfo| lfo.borrow_mut::<Lfo>()).transpose()?;
                let envelope_depth = envelope_depth.unwrap_or(0.0);
                let lfo_depth = lfo_depth.unwrap_or(0.0);
                if !envelope_depth.is_finite() || !lfo_depth.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "filter modulation depths must be finite".to_string(),
                    ));
                }
                this.render_filtered(
                    frequency,
                    sample_rate,
                    gain,
                    RenderModulation {
                        envelope: envelope.as_deref_mut(),
                        lfo: lfo.as_deref_mut(),
                        envelope_depth_octaves: envelope_depth.clamp(-8.0, 8.0),
                        lfo_depth_octaves: lfo_depth.clamp(-8.0, 8.0),
                    },
                );
                Ok(())
            },
        );
    }
}

pub fn install_synth_api(
    lua: &Lua,
    globals: &Table,
    params: Arc<PluginParams>,
    audio: Arc<AudioBridge>,
) -> mlua::Result<()> {
    let table = lua.create_table()?;

    table.set(
        "note_freq",
        lua.create_function(|_, note: f64| Ok(440.0 * 2.0_f64.powf((note - 69.0) / 12.0)))?,
    )?;

    table.set(
        "lfo",
        lua.create_function(|lua, (waveform, rate): (String, f64)| {
            lua.create_userdata(Lfo {
                waveform: Waveform::parse_lfo(&waveform)?,
                rate: rate.clamp(0.0, 20_000.0),
                sync_beats: None,
                tempo: 120.0,
                phase: 0.0,
                inverted: false,
            })
        })?,
    )?;

    let envelope_constructor = lua.create_function(
        |lua, (attack, decay, sustain, release): (f64, f64, f64, f64)| {
            lua.create_userdata(Envelope::new(attack, decay, sustain, release))
        },
    )?;
    table.set("env", envelope_constructor.clone())?;
    table.set("adsr", envelope_constructor)?;

    let current = params.clone();
    table.set(
        "set_mode",
        lua.create_function(move |_, mode: Option<String>| {
            let mode = match mode.as_deref() {
                None | Some("default") => None,
                Some("mono") => Some(VoiceMode::Mono),
                Some("poly") => Some(VoiceMode::Poly),
                Some(value) => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "unknown voice mode: {value}"
                    )));
                }
            };
            current.set_voice_mode_override(mode);
            Ok(())
        })?,
    )?;

    let current = params.clone();
    table.set(
        "set_max_voices",
        lua.create_function(move |_, voices: Option<usize>| {
            if let Some(voices) = voices {
                if voices == 0 {
                    return Err(mlua::Error::RuntimeError(
                        "max voices must be at least 1".to_string(),
                    ));
                }
                current.set_max_voices_override(Some(voices.min(MAX_VOICES)));
            } else {
                current.set_max_voices_override(None);
            }
            Ok(())
        })?,
    )?;

    table.set(
        "osc",
        lua.create_function(move |lua, waveform: String| {
            let waveform = Waveform::parse(&waveform)?;
            lua.create_userdata(Oscillator::new(audio.clone(), waveform))
        })?,
    )?;

    globals.set("synth", table)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_api_exposes_note_frequency_and_script_overrides() {
        let lua = Lua::new();
        let globals = lua.globals();
        let params = Arc::new(PluginParams::default());
        install_synth_api(&lua, &globals, params.clone(), Arc::new(AudioBridge::new()))
            .expect("install synth API");

        lua.load(
            r#"
            assert(math.abs(synth.note_freq(69) - 440) < 1e-9)
            synth.set_mode("mono")
            synth.set_max_voices(12)
            local osc = synth.osc("saw")
            osc:set_unison(3, 8, 0.65)
            osc:set_fm(2.0, 1.5)
            osc:set_pd(0.35)
            osc:set_rm(1.4, 0.5)
            osc:set_lowpass(1200, 0.4)
            osc:set_lowpass_2(6000, 0.2)
            osc:set_distortion("tube", 18, 12, 0.5, 0.1, 0.4, 8, 4)
            assert(not pcall(function()
                osc:set_distortion("invalid", 0, 0, 0, 0, 0, 8, 1)
            end))
            osc:set_pitch_drop(18, 0.03)
            osc:render(440, 44100, 0.25)

            local lfo = synth.lfo("sine", 1.0)
            assert(math.abs(lfo:value()) < 1e-9)
            assert(lfo:process(1000, 44100) > 0.0)
            local inverted_lfo = synth.lfo("saw", 1.0)
            inverted_lfo:set_inverted(true)
            assert(inverted_lfo:value() == 1.0)

            local synced_lfo = synth.lfo("saw", 1.0)
            synced_lfo:set_sync("1/8")
            synced_lfo:set_tempo(124)
            assert(math.abs(synced_lfo:rate() - (124 * 2 / 60)) < 1e-9)
            synced_lfo:set_sync("1/8d")
            assert(math.abs(synced_lfo:rate() - (124 / 60 / 0.75)) < 1e-9)
            synced_lfo:set_rate(2.0)
            assert(synced_lfo:rate() == 2.0)

            local env = synth.adsr(0.0, 0.0, 0.5, 0.0)
            env:gate(true)
            osc:render_env(440, 44100, 0.25, env)
            osc:render_filtered(440, 44100, 0.25, env, lfo, 3.0, 0.5)
            osc:clear_filter()
            local noise = synth.osc("noise")
            noise:render_env(1200, 44100, 0.1, env)
            synth.osc("pink"):render(440, 44100, 0.1)
            synth.osc("brown"):render(440, 44100, 0.1)
            assert(env:process(1, 44100) == 0.5)
            env:set_adsr(0.0, 0.0, 0.25, 0.0)
            assert(env:value() == 0.25)
            env:gate(false)
            assert(env:process(1, 44100) == 0.0)
            assert(env:done())
            "#,
        )
        .exec()
        .expect("synth assertions failed");

        assert_eq!(params.effective_voice_mode(), VoiceMode::Mono);
        assert_eq!(params.effective_max_voices(), 12);
    }

    #[test]
    fn lowpass_filter_reduces_high_frequency_energy() {
        let mut filter = FilterState::default();
        let sample_rate = 44_100.0;
        let mut input_energy = 0.0;
        let mut output_energy = 0.0;

        for sample in 0..8_820 {
            let input = (TAU * 10_000.0 * sample as f64 / sample_rate).sin();
            let output = filter.outputs(input, 500.0, sample_rate, 0.0).0;
            if sample > 1_000 {
                input_energy += input * input;
                output_energy += output * output;
            }
        }

        assert!(output_energy < input_energy * 0.1);
    }

    #[test]
    fn distortion_modes_produce_finite_bounded_curves() {
        for mode in [
            DistortionMode::Saturator,
            DistortionMode::Bitcrush,
            DistortionMode::Tube,
            DistortionMode::Tape,
            DistortionMode::Overdrive,
            DistortionMode::Fuzz,
            DistortionMode::Diode,
        ] {
            let output = distort_sample(mode, 1.4, 0.2, 0.6, 8);
            assert!(output.is_finite());
            assert!((-1.0..=1.0).contains(&output));
        }

        assert_eq!(
            distort_sample(DistortionMode::Bitcrush, 0.3, 0.0, 0.0, 2),
            0.5
        );
    }

    #[test]
    fn noise_colors_generate_finite_nonconstant_samples() {
        let mut white = NoiseState::new(1);
        let mut pink = NoiseState::new(2);
        let mut brown = NoiseState::new(3);
        let first_white = white.sample(Waveform::WhiteNoise);
        let second_white = white.sample(Waveform::WhiteNoise);
        assert_ne!(first_white, second_white);

        for _ in 0..10_000 {
            assert!(pink.sample(Waveform::PinkNoise).is_finite());
            assert!(brown.sample(Waveform::BrownNoise).is_finite());
        }
    }
}
