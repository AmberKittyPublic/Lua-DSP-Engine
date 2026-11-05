use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use nice_plug::prelude::*;

const MAX_MIDI_EVENTS: usize = 1024;

/// Scratch space shared between the audio thread and the Lua callbacks.
///
/// The pointers stored here are only valid while [`AudioBridge::publish`] and
/// [`AudioBridge::clear`] bracket the call into Lua. Lua runs synchronously on
/// the audio thread, so nothing is ever read from another thread.
pub struct BlockAccess {
    buffer: *mut Buffer<'static>,
    aux: *mut AuxiliaryBuffers<'static>,
    num_samples: usize,
    main_channels: usize,
    aux_channels: usize,
    peak: f32,
}

impl BlockAccess {
    /// # Safety
    ///
    /// The caller must guarantee that `buffer` and `aux` stay alive and are not
    /// otherwise aliased for as long as this value (or any copy of its raw
    /// pointers) is reachable.
    #[allow(clippy::unnecessary_cast)]
    pub unsafe fn new(buffer: &mut Buffer<'_>, aux: &mut AuxiliaryBuffers<'_>) -> Self {
        let buffer = buffer as *mut Buffer<'_> as *mut Buffer<'static>;
        let aux = aux as *mut AuxiliaryBuffers<'_> as *mut AuxiliaryBuffers<'static>;

        let num_samples = unsafe { (*buffer).samples() };
        let main_channels = unsafe { (*buffer).channels() };
        let aux_channels = unsafe {
            (*aux)
                .inputs
                .first()
                .map(|buffer| buffer.channels())
                .unwrap_or(0)
        };

        Self {
            buffer,
            aux,
            num_samples,
            main_channels,
            aux_channels,
            peak: 0.0,
        }
    }

    pub fn num_samples(&self) -> usize {
        self.num_samples
    }

    pub fn num_channels(&self) -> usize {
        self.main_channels
    }

    pub fn aux_channels(&self) -> usize {
        self.aux_channels
    }

    pub fn peak(&self) -> f32 {
        self.peak
    }

    pub fn clear(&mut self) {
        let buffer = unsafe { &mut *self.buffer };
        for channel in buffer.as_slice() {
            channel.fill(0.0);
        }
        self.peak = 0.0;
    }

    pub fn get(&mut self, channel: usize, sample: usize) -> Option<f32> {
        if channel >= self.main_channels || sample >= self.num_samples {
            return None;
        }

        let buffer = unsafe { &mut *self.buffer };
        Some(buffer.as_slice()[channel][sample])
    }

    pub fn set(&mut self, channel: usize, sample: usize, value: f32) {
        if channel >= self.main_channels || sample >= self.num_samples {
            return;
        }

        let buffer = unsafe { &mut *self.buffer };
        buffer.as_slice()[channel][sample] = value;
        self.peak = self.peak.max(value.abs());
    }

    pub fn add(&mut self, channel: usize, sample: usize, value: f32) {
        if channel >= self.main_channels || sample >= self.num_samples {
            return;
        }

        let buffer = unsafe { &mut *self.buffer };
        let output = &mut buffer.as_slice()[channel][sample];
        *output += value;
        self.peak = self.peak.max(output.abs());
    }

    pub fn gain(&mut self, channel: usize, gain: f32) {
        if channel >= self.main_channels {
            return;
        }

        let buffer = unsafe { &mut *self.buffer };
        let samples = &mut buffer.as_slice()[channel];
        let mut peak = self.peak;
        for sample in samples.iter_mut() {
            *sample *= gain;
            peak = peak.max(sample.abs());
        }
        self.peak = peak;
    }

    pub fn sc_get(&mut self, channel: usize, sample: usize) -> Option<f32> {
        if channel >= self.aux_channels || sample >= self.num_samples {
            return None;
        }

        let aux = unsafe { &mut *self.aux };
        Some(aux.inputs[0].as_slice()[channel][sample])
    }
}

pub struct AudioBridge {
    ptr: AtomicPtr<BlockAccess>,
    num_samples: AtomicU32,
    num_channels: AtomicU32,
    aux_channels: AtomicU32,
}

impl Default for AudioBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBridge {
    pub fn new() -> Self {
        Self {
            ptr: AtomicPtr::new(std::ptr::null_mut()),
            num_samples: AtomicU32::new(0),
            num_channels: AtomicU32::new(0),
            aux_channels: AtomicU32::new(0),
        }
    }

    pub fn publish(&self, access: &mut BlockAccess) {
        self.num_samples
            .store(access.num_samples() as u32, Ordering::Release);
        self.num_channels
            .store(access.num_channels() as u32, Ordering::Release);
        self.aux_channels
            .store(access.aux_channels() as u32, Ordering::Release);
        self.ptr.store(access as *mut _, Ordering::Release);
    }

    pub fn clear(&self) {
        self.ptr.store(std::ptr::null_mut(), Ordering::Release);
    }

    pub fn num_samples(&self) -> usize {
        self.num_samples.load(Ordering::Acquire) as usize
    }

    pub fn num_channels(&self) -> usize {
        self.num_channels.load(Ordering::Acquire) as usize
    }

    pub fn aux_channels(&self) -> usize {
        self.aux_channels.load(Ordering::Acquire) as usize
    }

    pub fn with_access<R>(&self, f: impl FnOnce(&mut BlockAccess) -> R) -> Option<R> {
        let ptr = self.ptr.load(Ordering::Acquire);
        if ptr.is_null() {
            None
        } else {
            Some(f(unsafe { &mut *ptr }))
        }
    }
}

#[derive(Default)]
pub struct MidiOut {
    events: Vec<NoteEvent<()>>,
}

impl MidiOut {
    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn push(&mut self, event: NoteEvent<()>) {
        if self.events.len() < MAX_MIDI_EVENTS {
            self.events.push(event);
        }
    }

    pub fn drain(&mut self) -> impl Iterator<Item = NoteEvent<()>> + '_ {
        self.events.drain(..)
    }
}
