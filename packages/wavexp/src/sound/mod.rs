mod custom;
mod noise;
mod note;

use crate::{
    ctx::{AppEvent, ContextMut, ContextRef, EditorAction},
    input::Button,
    sequencer::Sequencer,
};
pub use custom::*;
pub use noise::*;
pub use note::*;
use std::{
    fmt::{self, Display, Formatter},
    future::Future,
    mem::{replace, variant_count},
    num::NonZeroU32,
    ops::{Add, Deref, Div, Sub},
    rc::Rc,
};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wavexp_utils::{error::Result, ext::default, r32, r64, real::R32, real::R64};
use web_sys::{AudioBuffer, AudioBufferOptions, AudioNode, BaseAudioContext, File};
use yew::Html;
use yew_html_ext::html;

pub type MSecs = R64;
pub type Secs = R64;
pub type Beats = R64;

pub trait FromBeats {
    fn to_msecs(self, bps: Self) -> MSecs;
    fn to_secs(self, bps: Self) -> Secs;
    fn secs_to_beats(self, bps: Self) -> Beats;
}

impl FromBeats for Beats {
    fn to_secs(self, bps: Self) -> Secs {
        self / bps
    }
    fn to_msecs(self, bps: Self) -> MSecs {
        self * 1000u16 / bps
    }
    fn secs_to_beats(self, bps: Self) -> Beats {
        self * bps
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
// Invariant: `self.0 <= Self::MAX.0`
pub struct Note(u8);

impl Deref for Note {
    type Target = u8;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for Note {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.name().fmt(f)
    }
}

macro_rules! convert_rhs {
    (id $rhs:ident) => {
        $rhs
    };
    (try_from $rhs:ident) => {
        $rhs.try_into().ok()?
    };
    (try_neg $rhs:ident) => {
        $rhs.checked_neg()?
    };
    (try_neg_from $rhs:ident) => {
        $rhs.checked_neg()?.try_into().ok()?
    };
}

macro_rules! impl_arith {
    ($trait:ident :: $method:ident as $impl:path, $mode:ident: $($int:ty),+) => {
        $(
            impl $trait<$int> for Note {
                type Output = Option<Note>;

                fn $method(self, rhs: $int) -> Self::Output {
                    Self::new($impl(self.0, convert_rhs!($mode rhs))?)
                }
            }
        )+
    };
}

impl_arith!(Add::add as u8::checked_add_signed, id:           i8);
impl_arith!(Add::add as u8::checked_add_signed, try_from:     i16, i32, isize, i64);
impl_arith!(Add::add as u8::checked_add, id:                  u8);
impl_arith!(Add::add as u8::checked_add, try_from:            u16, u32, usize, u64);
impl_arith!(Sub::sub as u8::checked_add_signed, try_neg:      i8);
impl_arith!(Sub::sub as u8::checked_add_signed, try_neg_from: i16, i32, isize, i64);
impl_arith!(Sub::sub as u8::checked_sub, id:                  u8);
impl_arith!(Sub::sub as u8::checked_sub, try_from:            u16, u32, usize, u64);

impl Sub for Note {
    type Output = Option<u8>;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0.checked_sub(rhs.0)
    }
}

impl Note {
    pub const N_NOTES: usize = 36;
    pub const MAX: Note = Note(Self::N_NOTES as u8 - 1);
    pub const MID: Note = Note(Self::N_NOTES as u8 / 2);
    pub const FREQS: [R32; Self::N_NOTES] = [
        r32!(65.410), // C2
        r32!(69.300), // C#2
        r32!(73.420), // D2
        r32!(77.780), // D#2
        r32!(82.410), // E2
        r32!(87.310), // F2
        r32!(92.500), // F#2
        r32!(98.000), // G2
        r32!(103.83), // G#2
        r32!(110.00), // A2
        r32!(116.54), // A#2
        r32!(123.47), // B2
        r32!(130.81), // C3
        r32!(138.59), // C#3
        r32!(146.83), // D3
        r32!(155.56), // D#3
        r32!(164.81), // E3
        r32!(174.61), // F3
        r32!(185.00), // F#3
        r32!(196.00), // G3
        r32!(207.65), // G#3
        r32!(220.00), // A3
        r32!(233.08), // A#3
        r32!(246.94), // B3
        r32!(261.63), // C4
        r32!(277.18), // C#4
        r32!(293.66), // D4
        r32!(311.13), // D#4
        r32!(329.63), // E4
        r32!(349.23), // F4
        r32!(369.99), // F#4
        r32!(392.00), // G4
        r32!(415.30), // G#4
        r32!(440.00), // A4
        r32!(466.16), // A#4
        r32!(493.88), // B4
    ];

    pub const NAMES: [&'static str; Self::N_NOTES] = [
        "C2", "C#2", "D2", "D#2", "E2", "F2", "F#2", "G2", "G#2", "A2", "A#2", "B2", "C3", "C#3",
        "D3", "D#3", "E3", "F3", "F#3", "G3", "G#3", "A3", "A#3", "B3", "C4", "C#4", "D4", "D#4",
        "E4", "F4", "F#4", "G4", "G#4", "A4", "A#4", "B4",
    ];

    pub const fn new(index: u8) -> Option<Self> {
        if index <= Self::MAX.0 {
            Some(Self(index))
        } else {
            None
        }
    }

    pub const fn saturated(index: u8) -> Self {
        match Self::new(index) {
            Some(res) => res,
            None => Self::MAX,
        }
    }

    pub const fn index(&self) -> usize {
        self.0 as usize
    }

    pub fn freq(&self) -> R32 {
        unsafe { *Self::FREQS.get_unchecked(self.0 as usize) }
    }

    pub fn name(&self) -> &'static str {
        unsafe { Self::NAMES.get_unchecked(self.0 as usize) }
    }

    pub const fn recip(self) -> Self {
        Self(Self::MAX.0 - self.0)
    }

    pub fn pitch_coef(&self) -> R64 {
        r64!(self.0 as i8 - Self::MID.0 as i8).div(12u8).exp2()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioInputChanges {
    /// Make the input play backwards.
    pub reversed: bool,
    /// cut the input from the start.
    pub cut_start: Beats,
    /// cut the input from the end.
    pub cut_end: Beats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioInput {
    name: Rc<str>,
    duration: Secs,
    raw: AudioBuffer,
    raw_duration: Secs,
    pending_changes: AudioInputChanges,
    baked_changes: AudioInputChanges,
    baked: AudioBuffer,
}

impl AudioInput {
    pub fn new(name: Rc<str>, mut buffer: AudioBuffer) -> Result<Self> {
        if buffer.number_of_channels() != Sequencer::CHANNEL_COUNT {
            let new_buffer = AudioBuffer::new(
                AudioBufferOptions::new(buffer.length(), Sequencer::SAMPLE_RATE as f32)
                    .number_of_channels(Sequencer::CHANNEL_COUNT),
            )?;
            let main_ch = buffer.get_channel_data(0)?;
            for ch_id in 0..Sequencer::CHANNEL_COUNT as i32 {
                new_buffer.copy_to_channel(&main_ch, ch_id)?;
            }
            buffer = new_buffer;
        }
        let duration = buffer.duration().try_into()?;
        Ok(Self {
            name,
            duration,
            baked: buffer.clone(),
            raw: buffer,
            raw_duration: duration,
            pending_changes: default(),
            baked_changes: default(),
        })
    }

    pub fn from_file(file: File, sequencer: &Sequencer) -> impl Future<Output = Result<Self>> {
        Self::from_file_base(file, sequencer.audio_ctx().clone())
    }

    async fn from_file_base(file: File, audio_ctx: BaseAudioContext) -> Result<Self> {
        let raw = JsFuture::from(file.array_buffer()).await?.dyn_into()?;
        let buffer: AudioBuffer =
            JsFuture::from(audio_ctx.decode_audio_data(&raw)?).await?.dyn_into()?;
        Self::new(format!("File {:?}", file.name()).into(), buffer)
    }

    /// Name of the input, exists solely for the user's convenience.
    pub const fn name(&self) -> &Rc<str> {
        &self.name
    }
    /// Sets the name of the input, returning the old one.
    pub fn set_name(&mut self, name: Rc<str>) -> Rc<str> {
        replace(&mut self.name, name)
    }
    /// Duration of the raw buffer, unchanged since the moment the input was created.
    pub const fn raw_duration(&self) -> Secs {
        self.raw_duration
    }
    /// Duration of the buffer with all the requested changes baked in.
    pub const fn baked_duration(&self) -> Secs {
        self.duration
    }

    // Raw buffer, unchanged since the moment the input was created.
    pub const fn raw(&self) -> &AudioBuffer {
        &self.raw
    }

    /// Get a struct holding all the changes yet to be baked into the input.
    pub const fn changes(&self) -> AudioInputChanges {
        self.pending_changes
    }
    /// Get a mutable reference to a struct holding all the changes yet to be baked into the input.
    pub fn changes_mut(&mut self) -> &mut AudioInputChanges {
        &mut self.pending_changes
    }

    /// Bake all of the changes into a buffer that will be accessible through `.baked()` method.
    /// If an error occurs, the input will appear unbaked.
    pub fn bake(&mut self, bps: Beats) -> Result {
        if self.pending_changes == self.baked_changes {
            return Ok(());
        };
        let cut_start =
            (*self.pending_changes.cut_start.to_secs(bps) * Sequencer::SAMPLE_RATE as f64) as usize;
        let cut_end =
            (*self.pending_changes.cut_end.to_secs(bps) * Sequencer::SAMPLE_RATE as f64) as usize;
        let length = self.raw.length() - cut_start as u32 - cut_end as u32;
        self.baked = AudioBuffer::new(
            AudioBufferOptions::new(length, Sequencer::SAMPLE_RATE as f32)
                .number_of_channels(Sequencer::CHANNEL_COUNT),
        )?;

        // TODO: this doesn't affect anything for some reason.
        self.duration = R64::from(length) / Sequencer::SAMPLE_RATE;
        for i in 0..Sequencer::CHANNEL_COUNT {
            let mut data = self.raw.get_channel_data(i)?;
            if self.pending_changes.reversed {
                data.reverse();
            }
            self.baked.copy_to_channel(&data[cut_start..], i as i32)?;
        }

        Ok(self.baked_changes = self.pending_changes)
    }

    /// Buffer with all the requested changes baked in.
    /// If the there are unbaked changes, `None` is returned.
    pub fn baked(&self) -> Option<&AudioBuffer> {
        (self.pending_changes == self.baked_changes).then_some(&self.baked)
    }

    pub fn desc(&self, bps: Beats) -> String {
        format!("{}, {:.2} beats", self.name, self.duration.secs_to_beats(bps))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundType {
    Note,
    Noise,
    Custom,
}

impl SoundType {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Note => NoteSound::NAME,
            Self::Noise => NoiseSound::NAME,
            Self::Custom => CustomSound::NAME,
        }
    }
}

#[derive(Default, Debug, Clone)]
pub enum Sound {
    #[default]
    None,
    Note(NoteSound),
    Noise(NoiseSound),
    Custom(CustomSound),
}

impl Sound {
    pub const TYPES: [SoundType; variant_count::<Self>() - 1 /* None */] = [
        SoundType::Note,
        SoundType::Noise,
        SoundType::Custom
    ];

    pub fn new(sound_type: SoundType) -> Self {
        match sound_type {
            SoundType::Note => Self::Note(default()),
            SoundType::Noise => Self::Noise(default()),
            SoundType::Custom => Self::Custom(default()),
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::None => "Undefined",
            Self::Note(_) => NoteSound::NAME,
            Self::Noise(_) => NoiseSound::NAME,
            Self::Custom(_) => CustomSound::NAME,
        }
    }

    pub fn prepare(&mut self, bps: Beats) -> Result {
        match self {
            Sound::Custom(inner) => inner.prepare(bps),
            _ => Ok(()),
        }
    }

    pub fn play(&self, plug: &AudioNode, now: Secs, self_offset: Secs, bps: Beats) -> Result {
        match self {
            Self::None => Ok(()),
            Self::Note(inner) => inner.play(plug, now, self_offset, bps),
            Self::Noise(inner) => inner.play(plug, now, self_offset, bps),
            Self::Custom(inner) => inner.play(plug, now, self_offset, bps),
        }
    }

    pub fn len(&self, bps: Beats) -> Result<Beats> {
        match self {
            Self::None => Ok(r64!(1)),
            Self::Note(inner) => inner.len(),
            Self::Noise(inner) => inner.len(),
            Self::Custom(inner) => inner.len(bps),
        }
    }

    pub const fn rep_count(&self) -> NonZeroU32 {
        match self {
            Self::None => NonZeroU32::MIN,
            Self::Note(inner) => inner.rep_count(),
            Self::Noise(inner) => inner.rep_count(),
            Self::Custom(inner) => inner.rep_count(),
        }
    }

    pub fn params(&self, ctx: ContextRef, sequencer: &Sequencer) -> Html {
        match self {
            Self::None => {
                let emitter = ctx.event_emitter();
                html! {
                    <div class="horizontal-menu">
                        for x in Sound::TYPES {
                            <Button
                                name={x.name()}
                                onclick={emitter.reform(move |_| AppEvent::SetBlockType(x))}
                            >
                                <p>{ x.name() }</p>
                            </Button>
                        }
                    </div>
                }
            }

            Self::Note(inner) => inner.params(ctx),
            Self::Noise(inner) => inner.params(ctx),
            Self::Custom(inner) => inner.params(ctx, sequencer),
        }
    }

    pub fn handle_event(
        &mut self,
        event: &AppEvent,
        mut ctx: ContextMut,
        sequencer: &Sequencer,
        offset: Beats,
    ) -> Result {
        let r = &mut false;
        match self {
            Sound::None => match event {
                &AppEvent::SetBlockType(ty) => {
                    *self = Self::new(ty);
                    ctx.register_action(EditorAction::SetBlockType(ty))?;
                    ctx.emit_event(AppEvent::RedrawEditorPlane);
                }

                AppEvent::Redo(actions) => {
                    for action in actions.iter() {
                        if let &EditorAction::SetBlockType(ty) = action {
                            *self = Self::new(ty);
                            ctx.emit_event(AppEvent::RedrawEditorPlane);
                        }
                    }
                }

                _ => (),
            },

            Sound::Note(inner) => inner.handle_event(event, ctx, sequencer, r, offset)?,
            Sound::Noise(inner) => inner.handle_event(event, ctx, sequencer, r, offset)?,
            Sound::Custom(inner) => inner.handle_event(event, ctx, sequencer, r, offset)?,
        };
        if *r {
            *self = Self::None
        }
        Ok(())
    }
}
