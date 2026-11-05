use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use nice_plug::prelude::*;

const MAX_MIDI_EVENTS: usize = 1024;
const MAX_AUDIO_CHANNELS: usize = 8;

pub struct BlockAccess {
    main_ptrs: [*mut f32; MAX_AUDIO_CHANNELS],
    main_channels: usize,
    num_samples: usize,
    aux_ptrs: [*mut f32; MAX_AUDIO_CHANNELS],
    aux_channels: usize,
    peak: f32,
    channel_peaks: [f32; MAX_AUDIO_CHANNELS],
}

impl BlockAccess {
    /// # Safety
    /// Callers must ensure the slices are valid.
    pub unsafe fn from_slices(main: &mut [&mut [f32]], aux: Option<&mut [&mut [f32]]>) -> Self {
        let main_channels = main.len().min(MAX_AUDIO_CHANNELS);
        let num_samples = main.first().map(|slice| slice.len()).unwrap_or(0);
        let mut main_ptrs = [std::ptr::null_mut(); MAX_AUDIO_CHANNELS];
        for (ptr, slice) in main_ptrs.iter_mut().zip(main.iter_mut()) {
            *ptr = slice.as_mut_ptr();
        }
        let mut aux_ptrs = [std::ptr::null_mut(); MAX_AUDIO_CHANNELS];
        let aux_channels = aux.map_or(0, |aux| {
            for (ptr, slice) in aux_ptrs.iter_mut().zip(aux.iter_mut()) {
                *ptr = slice.as_mut_ptr();
            }
            aux.len().min(MAX_AUDIO_CHANNELS)
        });
        Self {
            main_ptrs,
            main_channels,
            num_samples,
            aux_ptrs,
            aux_channels,
            peak: 0.0,
            channel_peaks: [0.0; MAX_AUDIO_CHANNELS],
        }
    }

    /// # Safety
    /// Callers must ensure buffers are valid.
    pub unsafe fn new(buffer: &mut Buffer<'_>, aux: &mut AuxiliaryBuffers<'_>) -> Self {
        let main_slices = buffer.as_slice();
        let aux_slices = if aux.inputs.is_empty() {
            None
        } else {
            Some(aux.inputs[0].as_slice())
        };
        unsafe { Self::from_slices(main_slices, aux_slices) }
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

    pub fn channel_peak(&self, channel: usize) -> f32 {
        self.channel_peaks.get(channel).copied().unwrap_or(0.0)
    }

    pub fn clear(&mut self) {
        for &ptr in self.main_ptrs.iter().take(self.main_channels) {
            for sample in 0..self.num_samples {
                unsafe {
                    *ptr.add(sample) = 0.0;
                }
            }
        }
        self.peak = 0.0;
        self.channel_peaks.fill(0.0);
    }

    pub fn get(&mut self, channel: usize, sample: usize) -> Option<f32> {
        if channel >= self.main_channels || sample >= self.num_samples {
            return None;
        }
        Some(unsafe { *self.main_ptrs[channel].add(sample) })
    }

    pub fn set(&mut self, channel: usize, sample: usize, value: f32) {
        if channel >= self.main_channels || sample >= self.num_samples {
            return;
        }
        unsafe {
            *self.main_ptrs[channel].add(sample) = value;
        }
        let channel_peak = &mut self.channel_peaks[channel];
        *channel_peak = channel_peak.max(value.abs());
        self.peak = self.peak.max(*channel_peak);
    }

    pub fn add(&mut self, channel: usize, sample: usize, value: f32) {
        if channel >= self.main_channels || sample >= self.num_samples {
            return;
        }
        let output = unsafe {
            let slot = self.main_ptrs[channel].add(sample);
            *slot += value;
            *slot
        };
        let channel_peak = &mut self.channel_peaks[channel];
        *channel_peak = channel_peak.max(output.abs());
        self.peak = self.peak.max(*channel_peak);
    }

    pub fn gain(&mut self, channel: usize, gain: f32) {
        if channel >= self.main_channels {
            return;
        }
        let ptr = self.main_ptrs[channel];
        let mut channel_peak = 0.0f32;
        for sample in 0..self.num_samples {
            unsafe {
                *ptr.add(sample) *= gain;
                channel_peak = channel_peak.max((*ptr.add(sample)).abs());
            }
        }
        self.channel_peaks[channel] = channel_peak;
        self.peak = self.channel_peaks.iter().copied().fold(0.0, f32::max);
    }

    pub fn sc_get(&mut self, channel: usize, sample: usize) -> Option<f32> {
        if channel >= self.aux_channels || sample >= self.num_samples {
            return None;
        }
        Some(unsafe { *self.aux_ptrs[channel].add(sample) })
    }
}

#[cfg(test)]
mod tests {
    use super::BlockAccess;

    #[test]
    fn clear_only_touches_present_channels() {
        let mut left = [0.5f32, -0.25];
        let mut right = [0.75f32, -0.5];
        let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
        let mut access = unsafe { BlockAccess::from_slices(&mut channels, None) };

        access.clear();

        assert_eq!(left, [0.0, 0.0]);
        assert_eq!(right, [0.0, 0.0]);
        assert_eq!(access.num_channels(), 2);
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
