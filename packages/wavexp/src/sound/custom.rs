use super::NoteBlock;
use crate::{
    global::{AppAction, AppContext, AppEvent},
    input::{AudioInputButton, Counter, Cursor, GraphEditorCanvas, Slider},
    popup::Popup,
    sequencer::{PlaybackContext, Sequencer},
    sound::{AudioInput, Beats, FromBeats, Note, Secs},
    visual::{GraphEditor, GraphPoint},
};
use std::{
    cmp::Ordering,
    mem::{replace, transmute},
    num::NonZeroUsize,
    ops::{Range, RangeInclusive},
};
use wasm_bindgen::JsCast;
use wavexp_utils::{
    cell::{Shared, SharedAwareRefMut},
    js_function, r32, r64, AppError, AppResult, AppResultUtils, ArrayExt, ArrayFrom, OptionExt,
    RangeExt, R32, R64,
};
use web_sys::{AudioNode, Path2d};
use yew::{html, Html};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomBlock {
    offset: R64,
    pitch: Note,
}

impl PartialOrd for CustomBlock {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.offset.cmp(&other.offset))
    }
}

impl Ord for CustomBlock {
    fn cmp(&self, other: &Self) -> Ordering {
        self.offset.cmp(&other.offset)
    }
}

impl GraphPoint for CustomBlock {
    const EDITOR_NAME: &'static str = NoteBlock::EDITOR_NAME;
    const Y_BOUND: Range<R64> = NoteBlock::Y_BOUND;
    const SCALE_Y_BOUND: Range<R64> = NoteBlock::SCALE_Y_BOUND;
    const OFFSET_Y_BOUND: Range<R64> = NoteBlock::OFFSET_Y_BOUND;
    const Y_SNAP: R64 = NoteBlock::Y_SNAP;

    type Inner = ();
    type Y = Note;
    /// (sound block offset, number of repetitions of the pattern, audio duration)
    type VisualContext = (Beats, NonZeroUsize, Beats);

    fn create(_: &GraphEditor<Self>, [offset, y]: [R64; 2]) -> Self {
        Self {
            offset,
            pitch: Note::from_index(y.into()).recip(),
        }
    }

    fn inner(&self) -> &Self::Inner {
        &()
    }
    fn inner_mut(&mut self) -> &mut Self::Inner {
        unsafe { transmute(self) }
    }

    fn y(&self) -> &Self::Y {
        &self.pitch
    }
    fn y_mut(&mut self) -> &mut Self::Y {
        &mut self.pitch
    }

    fn loc(&self) -> [R64; 2] {
        [self.offset, self.pitch.recip().index().into()]
    }

    fn move_point(point: Result<&mut Self, &mut [R64; 2]>, delta: [R64; 2], _: bool) {
        match point {
            Ok(p) => {
                p.offset += delta[0];
                p.pitch -= delta[1].into();
            }
            Err(p) => {
                p[0] += delta[0];
                p[1] += delta[1];
            }
        }
    }

    fn in_hitbox(
        &self,
        area: &[RangeInclusive<R64>; 2],
        _: &AppContext,
        _: &Sequencer,
        (.., len): Self::VisualContext,
    ) -> AppResult<bool> {
        Ok(area[1]
            .clone()
            .map_bounds(usize::from)
            .contains(&self.pitch.recip().index())
            && (self.offset..=self.offset + len / self.pitch.pitch_coef()).overlap(&area[0]))
    }

    fn fmt_loc(loc: [R64; 2]) -> String {
        format!("{:.3}, {}", loc[0], Note::from_index(loc[1].into()).recip())
    }

    fn on_move(
        editor: &mut GraphEditor<Self>,
        ctx: &mut AppContext,
        _: Cursor,
        _: [R64; 2],
        point: Option<usize>,
    ) -> AppResult<()> {
        let Some(last) = editor.len().checked_sub(1) else {
            return Ok(());
        };
        Ok(
            if point.map_or_else(|| editor.selection().contains(&last), |x| x == last) {
                ctx.emit_event(AppEvent::RedrawEditorPlane)
            },
        )
    }

    fn on_redraw(
        editor: &mut GraphEditor<Self>,
        ctx: &AppContext,
        sequencer: &Sequencer,
        canvas_size: &[R64; 2],
        solid: &Path2d,
        dotted: &Path2d,
        (sb_offset, n_reps, len): Self::VisualContext,
    ) -> AppResult<()> {
        let bps = sequencer.bps();
        let len = len.secs_to_beats(bps);
        let step = &canvas_size.div(&editor.scale());
        let offset = &R64::array_from(editor.offset());
        dotted.rect(
            -10.0,
            Note::MID.index() as f64 * *step[1] - *offset[1],
            *canvas_size[0] * 2.0,
            *step[1],
        );
        for block in editor.iter() {
            let [x, y] = block.loc().mul(step).sub(offset);
            solid.rect(
                *x,
                *y,
                *len / *block.pitch.pitch_coef() * *step[0],
                *step[1],
            );
        }

        let total_len = editor
            .last()
            .map_or_default(|x| x.offset + len / x.pitch.pitch_coef());
        Ok(if let PlaybackContext::All(start) = sequencer.playback_ctx() && start.is_finite() {
            let progress = (ctx.frame() - start).secs_to_beats(bps) - sb_offset;
            if progress < total_len * n_reps {
                editor.force_redraw();
                let x = R64::new_or(progress, *progress % *total_len) * step[0] - offset[0];
                solid.move_to(*x, 0.0);
                solid.line_to(*x, *canvas_size[1]);
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct CustomSound {
    pattern: Shared<GraphEditor<CustomBlock>>,
    src: Option<Shared<AudioInput>>,
    volume: R32,
    attack: Beats,
    decay: Beats,
    sustain: R32,
    release: Beats,
    rep_count: NonZeroUsize,
    speed: R32,
}

impl Default for CustomSound {
    fn default() -> Self {
        Self {
            pattern: GraphEditor::new(vec![CustomBlock {
                offset: r64![0],
                pitch: Note::MID,
            }])
            .into(),
            src: None,
            volume: r32![1],
            attack: r64![0],
            decay: r64![0],
            sustain: r32![1],
            release: r64![0],
            rep_count: NonZeroUsize::MIN,
            speed: r32![1],
        }
    }
}

impl CustomSound {
    pub const NAME: &'static str = "Custom Audio";

    pub fn prepare(&mut self, bps: Beats) -> AppResult<()> {
        if let Some(src) = &self.src {
            src.get_mut()?.bake(bps)
        } else {
            Ok(())
        }
    }

    pub fn play(
        &self,
        plug: &AudioNode,
        now: Secs,
        self_offset: Secs,
        bps: Beats,
    ) -> AppResult<()> {
        let Some(src) = &self.src else { return Ok(()) };
        let src = src.get()?;
        let pat = self.pattern.get()?;
        let Some(last) = pat.last() else {
            return Ok(());
        };
        let len = src.baked_duration() / self.speed;
        let pat_len = last.offset.to_secs(bps) + len / last.pitch.pitch_coef();
        let ctx = plug.context();

        Ok(for rep in 0..self.rep_count.get() {
            for CustomBlock { offset, pitch } in pat.iter() {
                let coef = pitch.pitch_coef();
                let block = ctx.create_gain()?;
                let gain = block.gain();
                let start = now + self_offset + pat_len * rep + offset.to_secs(bps);
                let mut at = start;
                gain.set_value(0.0);
                at += self.attack.to_secs(bps);
                gain.linear_ramp_to_value_at_time(*self.volume, *at)?;
                at += self.decay.to_secs(bps);
                let sus = self.sustain * self.volume;
                gain.linear_ramp_to_value_at_time(*sus, *at)?;
                at = start + len / coef;
                gain.set_value_at_time(*sus, *at - *self.release.to_secs(bps))?;
                gain.linear_ramp_to_value_at_time(0.0, *at)?;

                let block_core = ctx.create_buffer_source()?;
                block_core.set_buffer(src.baked().to_app_result()?.into());
                block_core.playback_rate().set_value(*(self.speed * coef));
                block_core
                    .connect_with_audio_node(&block)?
                    .connect_with_audio_node(plug)?;
                block_core.start_with_when(*start)?;
                block_core.clone().set_onended(Some(&js_function!(|| {
                    block.disconnect().map_err(AppError::from).report();
                    block_core.disconnect().map_err(AppError::from).report();
                })));
            }
        })
    }

    pub fn len(&self, bps: Beats) -> AppResult<Beats> {
        Ok(if let Some(block) = self.pattern.get()?.last() && let Some(src) = &self.src {
            src.get()?.baked_duration().secs_to_beats(bps)
                / self.speed / block.pitch.pitch_coef()
                + block.offset
        } else {r64![0]})
    }

    pub fn rep_count(&self) -> NonZeroUsize {
        self.rep_count
    }

    pub fn params(&self, ctx: &AppContext, sequencer: &Sequencer) -> Html {
        let emitter = ctx.event_emitter();
        match ctx.selected_tab() {
            0 /* General */ => html!{<div id="inputs">
                <Slider key="custom-vol"
                setter={emitter.reform(|x| AppEvent::Volume(R32::from(x)))}
                name="Custom Audio Volume"
                initial={self.volume}/>
                <Counter key="custom-repcnt"
                setter={emitter.reform(|x| AppEvent::RepCount(NonZeroUsize::from(x)))}
                fmt={|x| format!("{x:.0}")}
                name="Number Of Pattern Repetitions"
                min={r64![1]}
                initial={self.rep_count}/>
                <Counter key="note-speed"
                setter={emitter.reform(|x| AppEvent::Speed(R32::from(x)))}
                fmt={|x| format!("{x:.2}x")}
                name="Playback speed"
                initial={self.speed}/>
                <AudioInputButton name="Audio input" help="Click to change"
                onclick={emitter.reform(|_| AppEvent::OpenPopup(Popup::ChooseInput))}
                playing={sequencer.playback_ctx().played_input().is_some()}
                bps={sequencer.bps()} {emitter} input={&self.src}/>
            </div>},

            1 /* Envelope */ => html!{<div id="inputs">
                <Counter key="custom-att"
                setter={emitter.reform(AppEvent::Attack)}
                name="Audio Attack Time" postfix="Beats"
                initial={self.attack}/>
                <Counter key="custom-dec"
                setter={emitter.reform(AppEvent::Decay)}
                name="Audio Decay Time" postfix="Beats"
                initial={self.decay}/>
                <Slider key="custom-sus"
                setter={emitter.reform(|x| AppEvent::Sustain(R32::from(x)))}
                name="Audio Sustain Level"
                initial={self.sustain}/>
                <Counter key="custom-rel"
                setter={emitter.reform(AppEvent::Release)}
                name="Audio Release Time" postfix="Beats"
                initial={self.release}/>
            </div>},

            2 /* Pattern */ => html!{
                <GraphEditorCanvas<CustomBlock> editor={&self.pattern} {emitter}/>
            },

            tab_id => html!{<p style="color:red">{format!("Invalid tab ID: {tab_id}")}</p>}
        }
    }

    /// `reset_sound` is set to `false` initially,
    /// if set to true, resets the sound block to an `Undefined` type
    pub fn handle_event(
        &mut self,
        event: &AppEvent,
        ctx: &mut AppContext,
        sequencer: &SharedAwareRefMut<'_, Sequencer>,
        reset_sound: &mut bool,
        offset: Beats,
    ) -> AppResult<()> {
        Ok(match *event {
            AppEvent::Volume(to) => ctx.register_action(AppAction::SetVolume {
                from: replace(&mut self.volume, to),
                to,
            }),

            AppEvent::Attack(to) => ctx.register_action(AppAction::SetAttack {
                from: replace(&mut self.attack, to),
                to,
            }),

            AppEvent::Decay(to) => ctx.register_action(AppAction::SetDecay {
                from: replace(&mut self.decay, to),
                to,
            }),

            AppEvent::Sustain(to) => ctx.register_action(AppAction::SetSustain {
                from: replace(&mut self.sustain, to),
                to,
            }),

            AppEvent::Release(to) => ctx.register_action(AppAction::SetRelease {
                from: replace(&mut self.release, to),
                to,
            }),

            AppEvent::RepCount(to) => {
                ctx.register_action(AppAction::SetRepCount {
                    from: replace(&mut self.rep_count, to),
                    to,
                });
                ctx.emit_event(AppEvent::RedrawEditorPlane);
            }

            AppEvent::Speed(to) => {
                ctx.register_action(AppAction::SetSpeed {
                    from: replace(&mut self.speed, to),
                    to,
                });
                ctx.emit_event(AppEvent::RedrawEditorPlane);
            }

            AppEvent::AddInput(ref to) | AppEvent::SelectInput(ref to) => {
                ctx.register_action(AppAction::SelectInput {
                    from: self.src.clone(),
                    to: Some(to.clone()),
                });
                self.src = Some(to.clone());
                ctx.emit_event(AppEvent::RedrawEditorPlane)
            }

            AppEvent::Undo(ref actions) => {
                let mut pat = self.pattern.get_mut()?;
                for action in actions.iter() {
                    match *action {
                        AppAction::SetBlockType(_) => {
                            *reset_sound = true;
                            break;
                        }

                        AppAction::SetVolume { from, .. } => self.volume = from,

                        AppAction::SetAttack { from, .. } => self.attack = from,

                        AppAction::SetDecay { from, .. } => self.decay = from,

                        AppAction::SetSustain { from, .. } => self.sustain = from,

                        AppAction::SetRelease { from, .. } => self.release = from,

                        AppAction::SetRepCount { from, .. } => {
                            self.rep_count = from;
                            ctx.emit_event(AppEvent::RedrawEditorPlane)
                        }

                        AppAction::SetSpeed { from, .. } => {
                            self.speed = from;
                            ctx.emit_event(AppEvent::RedrawEditorPlane)
                        }

                        AppAction::SelectInput { ref from, .. } => {
                            self.src = from.clone();
                            ctx.emit_event(AppEvent::RedrawEditorPlane)
                        }

                        _ => (),
                    }
                }
                if ctx.selected_tab() == 2 {
                    pat.handle_event(event, ctx, sequencer, || {
                        (
                            offset,
                            self.rep_count,
                            self.src
                                .as_ref()
                                .and_then(|x| x.get().ok())
                                .map_or_default(|x| x.baked_duration() / self.speed),
                        )
                    })?;
                }
            }

            AppEvent::Redo(ref actions) => {
                let mut pat = self.pattern.get_mut()?;
                for action in actions.iter() {
                    match *action {
                        AppAction::SetVolume { to, .. } => self.volume = to,

                        AppAction::SetAttack { to, .. } => self.attack = to,

                        AppAction::SetDecay { to, .. } => self.decay = to,

                        AppAction::SetSustain { to, .. } => self.sustain = to,

                        AppAction::SetRelease { to, .. } => self.release = to,

                        AppAction::SetRepCount { to, .. } => {
                            self.rep_count = to;
                            ctx.emit_event(AppEvent::RedrawEditorPlane)
                        }

                        AppAction::SetSpeed { to, .. } => {
                            self.speed = to;
                            ctx.emit_event(AppEvent::RedrawEditorPlane)
                        }

                        AppAction::SelectInput { ref to, .. } => {
                            self.src = to.clone();
                            ctx.emit_event(AppEvent::RedrawEditorPlane)
                        }

                        _ => (),
                    }
                }
                if ctx.selected_tab() == 2 {
                    pat.handle_event(event, ctx, sequencer, || {
                        (
                            offset,
                            self.rep_count,
                            self.src
                                .as_ref()
                                .and_then(|x| x.get().ok())
                                .map_or_default(|x| x.baked_duration() / self.speed),
                        )
                    })?;
                }
            }

            _ => {
                if ctx.selected_tab() == 2 {
                    self.pattern
                        .get_mut()?
                        .handle_event(event, ctx, sequencer, || {
                            (
                                offset,
                                self.rep_count,
                                self.src
                                    .as_ref()
                                    .and_then(|x| x.get().ok())
                                    .map_or_default(|x| x.baked_duration() / self.speed),
                            )
                        })?;
                }
            }
        })
    }
}