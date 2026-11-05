use nice_plug::midi::{Channel, Key, NoteEvent, VoiceID};

use crate::params::{MAX_VOICES, VoiceMode};

#[derive(Debug, Clone, Copy)]
pub struct Voice {
    pub id: u64,
    pub note: u8,
    pub channel: u8,
    pub velocity: f32,
    pub gate: bool,
    pub age: u64,
    host_voice_id: VoiceID,
}

#[derive(Debug, Clone, Copy)]
struct HeldNote {
    note: u8,
    channel: u8,
    velocity: f32,
    age: u64,
    host_voice_id: VoiceID,
}

pub struct VoiceManager {
    voices: Vec<Voice>,
    mono_notes: Vec<HeldNote>,
    terminated: Vec<u64>,
    mode: VoiceMode,
    max_voices: usize,
    next_id: u64,
    age: u64,
}

impl Default for VoiceManager {
    fn default() -> Self {
        Self {
            voices: Vec::with_capacity(MAX_VOICES),
            mono_notes: Vec::with_capacity(MAX_VOICES),
            terminated: Vec::with_capacity(MAX_VOICES),
            mode: VoiceMode::Poly,
            max_voices: 8,
            next_id: 1,
            age: 0,
        }
    }
}

impl VoiceManager {
    pub fn voices(&self) -> &[Voice] {
        &self.voices
    }

    pub fn contains(&self, id: u64) -> bool {
        self.voices.iter().any(|voice| voice.id == id)
    }

    pub fn take_terminated(&mut self, output: &mut [Option<u64>]) -> usize {
        let count = output.len().min(self.terminated.len());
        for (slot, id) in output.iter_mut().zip(self.terminated.drain(..count)) {
            *slot = Some(id);
        }
        count
    }

    pub fn configure(&mut self, mode: VoiceMode, max_voices: usize) {
        let max_voices = max_voices.clamp(1, MAX_VOICES);
        if self.mode != mode {
            match mode {
                VoiceMode::Mono => {
                    let newest = self
                        .voices
                        .iter()
                        .filter(|voice| voice.gate)
                        .max_by_key(|voice| voice.age)
                        .cloned();
                    let terminated = &mut self.terminated;
                    self.voices.retain(|voice| {
                        let keep =
                            !voice.gate || newest.is_some_and(|active| active.id == voice.id);
                        if !keep {
                            terminated.push(voice.id);
                        }
                        keep
                    });
                    self.mono_notes.clear();
                    if let Some(voice) = newest {
                        self.mono_notes.push(HeldNote {
                            note: voice.note,
                            channel: voice.channel,
                            velocity: voice.velocity,
                            age: voice.age,
                            host_voice_id: voice.host_voice_id,
                        });
                        self.voices.push(voice);
                    }
                }
                VoiceMode::Poly => self.mono_notes.clear(),
            }
            self.mode = mode;
        }

        self.max_voices = max_voices;
        self.trim_to_limit();
    }

    pub fn handle_event<S>(&mut self, event: &NoteEvent<S>) {
        match event {
            NoteEvent::NoteOn {
                voice_id,
                channel,
                key,
                velocity,
                ..
            } if *velocity > 0.0 => {
                if let Some(note) = key.number() {
                    self.note_on(*voice_id, channel.number().unwrap_or(0), note, *velocity);
                }
            }
            NoteEvent::NoteOn {
                voice_id,
                channel,
                key,
                ..
            }
            | NoteEvent::NoteOff {
                voice_id,
                channel,
                key,
                ..
            } => {
                self.note_off(*voice_id, *channel, *key);
            }
            NoteEvent::Choke {
                voice_id,
                channel,
                key,
                ..
            } => self.choke(*voice_id, *channel, *key),
            _ => {}
        }
    }

    pub fn finish(&mut self, id: u64) {
        if let Some(index) = self.voices.iter().position(|voice| voice.id == id) {
            self.voices.remove(index);
            self.terminated.push(id);
        }
        if self.mode == VoiceMode::Mono {
            self.mono_notes.clear();
        }
    }

    fn note_on(&mut self, host_voice_id: VoiceID, channel: u8, note: u8, velocity: f32) {
        self.age = self.age.wrapping_add(1);
        if self.mode == VoiceMode::Mono {
            self.mono_notes.push(HeldNote {
                note,
                channel,
                velocity,
                age: self.age,
                host_voice_id,
            });

            if let Some(voice) = self.voices.iter_mut().find(|voice| voice.gate) {
                voice.note = note;
                voice.channel = channel;
                voice.velocity = velocity;
                voice.host_voice_id = host_voice_id;
                voice.age = self.age;
            } else if let Some(voice) = self.voices.first_mut() {
                voice.note = note;
                voice.channel = channel;
                voice.velocity = velocity;
                voice.gate = true;
                voice.host_voice_id = host_voice_id;
                voice.age = self.age;
            } else {
                let id = self.allocate_id();
                self.voices.push(Voice {
                    id,
                    note,
                    channel,
                    velocity,
                    gate: true,
                    age: self.age,
                    host_voice_id,
                });
            }
            return;
        }

        self.make_room();
        let id = self.allocate_id();
        self.voices.push(Voice {
            id,
            note,
            channel,
            velocity,
            gate: true,
            age: self.age,
            host_voice_id,
        });
    }

    fn note_off(&mut self, voice_id: VoiceID, channel: Channel, key: Key) {
        if self.mode == VoiceMode::Mono {
            if let Some(index) = self.mono_notes.iter().rposition(|held| {
                matches_note(
                    held.note,
                    held.channel,
                    held.host_voice_id,
                    voice_id,
                    channel,
                    key,
                )
            }) {
                self.mono_notes.remove(index);
            }

            let latest = self.mono_notes.iter().max_by_key(|held| held.age).cloned();
            if let Some(voice) = self.voices.iter_mut().find(|voice| voice.gate) {
                if let Some(held) = latest {
                    voice.note = held.note;
                    voice.channel = held.channel;
                    voice.velocity = held.velocity;
                    voice.host_voice_id = held.host_voice_id;
                    self.age = self.age.wrapping_add(1);
                    voice.age = self.age;
                } else {
                    voice.gate = false;
                }
            }
            return;
        }

        if let Some(voice) = self.voices.iter_mut().rev().find(|voice| {
            voice.gate
                && matches_note(
                    voice.note,
                    voice.channel,
                    voice.host_voice_id,
                    voice_id,
                    channel,
                    key,
                )
        }) {
            voice.gate = false;
        }
    }

    fn choke(&mut self, voice_id: VoiceID, channel: Channel, key: Key) {
        self.mono_notes.retain(|held| {
            !matches_note(
                held.note,
                held.channel,
                held.host_voice_id,
                voice_id,
                channel,
                key,
            )
        });

        if self.mode == VoiceMode::Mono {
            if let Some(held) = self.mono_notes.iter().max_by_key(|held| held.age).copied() {
                if let Some(voice) = self.voices.first_mut() {
                    voice.note = held.note;
                    voice.channel = held.channel;
                    voice.velocity = held.velocity;
                    voice.host_voice_id = held.host_voice_id;
                    voice.gate = true;
                }
            } else {
                for voice in self.voices.drain(..) {
                    self.terminated.push(voice.id);
                }
            }
        } else {
            let mut index = 0;
            while index < self.voices.len() {
                if matches_note(
                    self.voices[index].note,
                    self.voices[index].channel,
                    self.voices[index].host_voice_id,
                    voice_id,
                    channel,
                    key,
                ) {
                    let voice = self.voices.remove(index);
                    self.terminated.push(voice.id);
                } else {
                    index += 1;
                }
            }
        }
    }

    fn make_room(&mut self) {
        if self.voices.len() >= self.max_voices {
            let oldest = self
                .voices
                .iter()
                .enumerate()
                .min_by_key(|(_, voice)| (voice.gate, voice.age))
                .map(|(index, _)| index)
                .unwrap_or(0);
            let voice = self.voices.remove(oldest);
            self.terminated.push(voice.id);
        }
    }

    fn trim_to_limit(&mut self) {
        while self.voices.len() > self.max_voices {
            let oldest = self
                .voices
                .iter()
                .enumerate()
                .min_by_key(|(_, voice)| (voice.gate, voice.age))
                .map(|(index, _)| index)
                .unwrap_or(0);
            let voice = self.voices.remove(oldest);
            self.terminated.push(voice.id);
        }
        if self.mode == VoiceMode::Mono && self.voices.len() > 1 {
            let newest = self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| voice.gate)
                .max_by_key(|(_, voice)| voice.age)
                .map(|(index, _)| index);
            if let Some(newest) = newest {
                let voice = self.voices.remove(newest);
                for dropped in self.voices.drain(..) {
                    self.terminated.push(dropped.id);
                }
                self.voices.push(voice);
            }
        }
    }

    fn allocate_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }
}

fn matches_note(
    note: u8,
    voice_channel: u8,
    host_voice_id: VoiceID,
    event_voice_id: VoiceID,
    event_channel: Channel,
    event_key: Key,
) -> bool {
    if let VoiceID::ID(event_id) = event_voice_id {
        return host_voice_id == VoiceID::ID(event_id);
    }

    event_key
        .number()
        .is_none_or(|event_note| event_note == note)
        && event_channel
            .number()
            .is_none_or(|event_channel| event_channel == voice_channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_on(note: u8, velocity: f32) -> NoteEvent<()> {
        NoteEvent::NoteOn {
            timing: 0,
            voice_id: VoiceID::Wildcard,
            channel: Channel::Number(0),
            key: Key::Number(note),
            velocity,
        }
    }

    fn note_off(note: u8) -> NoteEvent<()> {
        NoteEvent::NoteOff {
            timing: 0,
            voice_id: VoiceID::Wildcard,
            channel: Channel::Number(0),
            key: Key::Number(note),
            velocity: 0.0,
        }
    }

    #[test]
    fn poly_mode_allocates_and_releases_multiple_voices() {
        let mut voices = VoiceManager::default();
        voices.configure(VoiceMode::Poly, 8);
        voices.handle_event(&note_on(60, 0.7));
        voices.handle_event(&note_on(64, 0.8));
        assert_eq!(voices.voices().len(), 2);

        voices.handle_event(&note_off(60));
        assert!(!voices.voices()[0].gate);
        assert!(voices.voices()[1].gate);
    }

    #[test]
    fn mono_mode_returns_to_the_previous_held_note() {
        let mut voices = VoiceManager::default();
        voices.configure(VoiceMode::Mono, 8);
        voices.handle_event(&note_on(60, 0.7));
        let id = voices.voices()[0].id;
        voices.handle_event(&note_on(64, 0.8));
        assert_eq!(voices.voices().len(), 1);
        assert_eq!(voices.voices()[0].id, id);
        assert_eq!(voices.voices()[0].note, 64);

        voices.handle_event(&note_off(64));
        assert_eq!(voices.voices()[0].note, 60);
        assert!(voices.voices()[0].gate);
    }

    #[test]
    fn max_voice_limit_steals_oldest_voice() {
        let mut voices = VoiceManager::default();
        voices.configure(VoiceMode::Poly, 1);
        voices.handle_event(&note_on(60, 0.7));
        voices.handle_event(&note_on(64, 0.8));
        assert_eq!(voices.voices().len(), 1);
        assert_eq!(voices.voices()[0].note, 64);
    }
}
