use std::sync::Arc;
use std::time::Instant;

use lua_plugin_core::{EditorHost, HostRole, LuaEditorType, LuaHost, PluginKind, PluginParams};
use nice_plug::prelude::*;

pub struct LuaAudioFx {
    params: Arc<PluginParams>,
    host: Option<LuaHost>,
    editor: EditorHost,
}

impl Default for LuaAudioFx {
    fn default() -> Self {
        let params = Arc::new(PluginParams::default());
        params.set_oversampling_factor(1);
        let editor = EditorHost::new(params.clone(), PluginKind::AudioFx);

        Self {
            params,
            host: None,
            editor,
        }
    }
}

impl Plugin for LuaAudioFx {
    const NAME: &'static str = "Lumina DSP Audio FX";
    const VENDOR: &'static str = "DayDRM software";
    const URL: &'static str = "https://example.com";
    const EMAIL: &'static str = "noreply@example.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            aux_input_ports: &[new_nonzero_u32(1)],
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type Editor = LuaEditorType;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        self.editor.create()
    }

    fn activate(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        let sample_rate = buffer_config.sample_rate;
        self.params.set_sample_rate(sample_rate);
        self.host = Some(LuaHost::new(
            self.params.clone(),
            PluginKind::AudioFx,
            HostRole::Audio,
            sample_rate,
        ));

        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let measure = self.params.debug_latency_enabled();
        let start = if measure { Some(Instant::now()) } else { None };
        if let Some(host) = self.host.as_mut() {
            let transport = context.transport();
            let sample_rate = transport.sample_rate;
            let tempo = transport.tempo;
            let playing = transport.playing;
            let pos_beats = transport.pos_beats;

            host.process_audio(buffer, aux, sample_rate, tempo, playing, pos_beats);
        }
        if let Some(start) = start {
            self.params
                .set_last_processing_latency_ms(start.elapsed().as_secs_f32() * 1000.0);
        }

        if self.editor.is_open() {
            self.editor.request_repaint();
        }

        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {
        self.host = None;
    }
}

impl ClapPlugin for LuaAudioFx {
    const CLAP_ID: &'static str = "com.lua-plugin-host.audio-fx";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("A scriptable audio effect powered by Luau");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Utility,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for LuaAudioFx {
    const VST3_CLASS_ID: [u8; 16] = *b"LuaAudioFx000001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Stereo];
}

nice_export_clap!(LuaAudioFx);
nice_export_vst3!(LuaAudioFx);
