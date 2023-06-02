use std::{
    iter::{Iterator, successors as succ},
    slice::from_raw_parts,
    mem::{Discriminant, discriminant},
    ops::Add, rc::Rc
};
use js_sys::Array as JsArray;
use web_sys::{HtmlCanvasElement, AnalyserNode as JsAnalyserNode, ImageData as JsImageData, MouseEvent as JsMouseEvent, HtmlElement};
use wasm_bindgen::{Clamped as JsClamped, JsValue};
use yew::{TargetCast, NodeRef, html, Html};
use crate::{
    utils::{Check, SliceExt, Point,
        JsResult, HtmlCanvasExt, JsResultUtils, R64, HitZone, OptionExt,
        HtmlElementExt, 
        Pipe, Tee, Rect, document, Take},
    loc, input::{ParamId, Slider, Switch}, sequencer::PatternBlock, sound::Beats, r64, js_log
};

pub struct EveryNth<'a, T> {
    iter: &'a [T],
    n: usize,
    state: usize
}

impl<'a, T> Iterator for EveryNth<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		let len = self.iter.len();
		let res = self.iter.get(self.state);
        self.state = (self.state + self.n)
            .check_not_in(len .. (len + self.n).saturating_sub(1))
            .unwrap_or_else(|x| x - len + 1);
        res
    }
}

pub struct EveryNthMut<'a, T> {
    iter: &'a mut [T],
    n: usize,
    state: usize
}

impl<'a, T> Iterator for EveryNthMut<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<&'a mut T> {
		let len = self.iter.len();
		let res = self.iter.get_mut(self.state)
            .map(|x| unsafe{(x as *mut T).as_mut().unwrap_unchecked()});
        // the abomination above is there solely because such a trivial task as
        // a lending iterator over mutable data can't be done any other way
        self.state = (self.state + self.n)
            .check_not_in(len .. (len + self.n).saturating_sub(1))
            .unwrap_or_else(|x| x - len + 1);
        res
    }
}

pub trait ToEveryNth<T> {
    fn every_nth(&self, n: usize) -> EveryNth<'_, T>;
    fn every_nth_mut(&mut self, n: usize) -> EveryNthMut<'_, T>;
}

impl<T> ToEveryNth<T> for [T] {
    #[inline] fn every_nth(&self, n: usize) -> EveryNth<'_, T> {
        EveryNth {iter: self, n, state: 0}
    }
    #[inline] fn every_nth_mut(&mut self, n: usize) -> EveryNthMut<'_, T> {
        EveryNthMut {iter: self, n, state: 0}
    }
}


#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgba {
	pub r: u8,
	pub g: u8,
	pub b: u8,
	pub a: u8
}

impl From<u32> for Rgba {
	fn from(val: u32) -> Self {
		Self {
			r: (val >> 24) as u8,
			g: ((val >> 16) & 0xFF) as u8,
			b: ((val >>  8) & 0xFF) as u8,
			a: (val & 0xFF) as u8}
	}
}

impl From<Rgba> for u32 {
	fn from(val: Rgba) -> Self {
		val.a as u32
		| (val.b as u32) << 8
		| (val.g as u32) << 16
		| (val.r as u32) << 24
	}
}

impl std::fmt::Display for Rgba {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
	}
}

fn interp<const N: usize>(colours: &[Rgba; N], index: u8) -> Rgba {
	let index = (index as usize * (N - 1)) as f32 / 255.0;
	let lower = colours.get_saturating(index.floor() as usize);
	let upper = colours.get_saturating(index.ceil() as usize);
	let weight = (index / (N - 1) as f32).fract();
	let weight_recip = 1.0 - weight;
	Rgba {
		r: (lower.r as f32 * weight_recip + upper.r as f32 * weight) as u8,
		g: (lower.g as f32 * weight_recip + upper.g as f32 * weight) as u8,
		b: (lower.b as f32 * weight_recip + upper.b as f32 * weight) as u8,
		a: (lower.a as f32 * weight_recip + upper.a as f32 * weight) as u8}
}

pub struct SoundVisualiser {
	out_data: Vec<Rgba>,
	in_data: Vec<u8>,
    gradient: Vec<Rgba>,
    canvas: NodeRef,
    width: u32, height: u32
}

impl SoundVisualiser {
	pub const FG: Rgba = Rgba{r:0x00, g:0x69, b:0xE1, a:0xFF};
	pub const BG: Rgba = Rgba{r:0x18, g:0x18, b:0x18, a:0xFF};
	pub fn new() -> JsResult<Self> {
		Ok(Self{out_data: vec![], in_data: vec![],
            gradient: (0 ..= u8::MAX)
                .map(|i| interp(&[Self::BG, Self::FG], i))
                .collect(),
			width: 0, height: 0, canvas: NodeRef::default()})
	}

    #[inline] pub fn canvas(&self) -> &NodeRef {
        &self.canvas
    }

    pub fn handle_resize(&mut self) -> JsResult<()> {
        let canvas: HtmlCanvasElement = self.canvas.cast().to_js_result(loc!())?;
        let [w, h] = canvas.client_size().map(|x| x as u32);
        canvas.set_width(w);
        canvas.set_height(h);
        self.width = w;
        self.height = h;
        self.in_data.resize(w as usize, 0);
        self.out_data.resize(w as usize * w as usize, Self::BG);
        Ok(())
    }

    // TODO: make it actually work
	pub fn poll(&mut self, input: Option<&JsAnalyserNode>) -> JsResult<()> {
		// TODO: correctly readjust the graph when shrinked in the UI
        let canvas: HtmlCanvasElement = self.canvas.cast().to_js_result(loc!())?;
        canvas.sync();
        let (new_width, new_height) = (canvas.width(), canvas.height());
        if new_width * new_height != self.width * self.height {
            self.width = new_width;
            self.height = new_height;
            self.in_data.resize(new_height as usize, 0);
            self.out_data.resize(new_width as usize * new_height as usize, Self::BG);
        }

        if let Some(input) = input {
            let len = self.out_data.len();
            self.out_data.copy_within(.. len - self.height as usize, self.height as usize);
            input.get_byte_frequency_data(&mut self.in_data);
            for (&src, dst) in self.in_data.iter().zip(self.out_data.every_nth_mut(self.width as usize)) {
                *dst = unsafe {*self.gradient.get_unchecked(src as usize)};
            }
            let out = JsClamped(unsafe{from_raw_parts(
                self.out_data.as_ptr().cast::<u8>(),
                self.out_data.len() * 4)});
            canvas.get_2d_context(loc!())?.put_image_data(
                    &JsImageData::new_with_u8_clamped_array(out, self.width).add_loc(loc!())?,
                    0.0, 0.0).add_loc(loc!())?;
        }

        Ok(())
	}
}


#[derive(Debug, Clone, Copy)]
pub struct CanvasEvent {
    pub point: Point,
    pub left: bool,
    pub shift: bool
}

impl TryFrom<&JsMouseEvent> for CanvasEvent {
    type Error = JsValue;
    fn try_from(value: &JsMouseEvent) -> Result<Self, Self::Error> {
        let canvas: HtmlCanvasElement = value.target_dyn_into().to_js_result(loc!())?;
        let point = Point{x: value.offset_x(), y: value.offset_y()}
            .normalise(canvas.client_rect(), canvas.rect());
        Ok(Self{point, left: value.buttons() & 1 == 1, shift: value.shift_key()})
    }
}

/*pub struct GraphHandler {
    canvas: NodeRef,
    ratio: R32,
    event: Option<Option<CanvasEvent>>
}

impl GraphHandler {
    #[inline] pub fn new() -> JsResult<Self> {
        Ok(Self{canvas: NodeRef::default(), ratio: R32::INFINITY, event: None})
    }

    #[inline] pub fn canvas(&self) -> &NodeRef {
        &self.canvas
    }

    #[inline] pub fn set_event(&mut self, event: CanvasEvent) {
        self.event = Some(Some(event));
    }

    /// the returned `bool` indicates whether the selected element's editor window should be
    /// rerendered
    pub fn set_param(&mut self, id: ParamId, _value: R64) -> bool {
        if let ParamId::Select(_) = id {
            self.event = Some(None);
            true
        } else {false}
    }

    #[inline] pub fn poll(&mut self, element: Option<&mut SoundGen>) -> JsResult<()> {
        if let Some((Some(spec), element)) = element.map(|x| (x.graph_spec(), x)) {
            let canvas = self.canvas.cast::<HtmlCanvasElement>().to_js_result(loc!())?;
            if spec.ratio != self.ratio {
                if *self.ratio == 0.0 {
                    self.event = self.event.or(None);
                }
                canvas.set_height((canvas.width() as f32 * *spec.ratio) as u32);
                self.ratio = spec.ratio;
            }
            if !spec.interactive {
                self.event = None;
            }
            if let Some(event) = self.event.take().or_else(|| spec.force_redraw.then_some(None)) {
                let ctx = canvas.get_2d_context(loc!())?;
                let (w, h) = (canvas.width(), canvas.height());
                ctx.set_fill_style(&"#181818".into());
                ctx.set_stroke_style(&"#0069E1".into());
                ctx.set_line_width(3.0);
                ctx.fill_rect(0.0, 0.0, w as f64, h as f64);
                ctx.begin_path();
                element.graph(w, h, &ctx, event).add_loc(loc!())?;
                ctx.stroke();
            }
        } else if *self.ratio != 0.0 {
            self.ratio = r32![0.0];
            self.canvas.cast::<HtmlCanvasElement>().to_js_result(loc!())?.set_height(0);
        }
        Ok(())
    }
}*/

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    None,
    HoverPlane,
    HoverElement(usize),
    MovePlane(i32),
    MoveElement(usize, Point),
}

pub struct EditorPlaneHandler {
    canvas: NodeRef,
    selected_id: Option<usize>,
    focus: Focus,
    last_focus: Discriminant<Focus>,
    offset: Point,
    redraw: bool,
    solid_line: JsValue,
    dotted_line: JsValue,
    scale_x: Beats,
    scale_y: i32,
    snap_step: R64
}

impl EditorPlaneHandler {
    const FONT: &str = "20px consolas";
    const BG_STYLE: &str = "#232328";
    const MG_STYLE: &str = "#333338";
    const FG_STYLE: &str = "#0069E1";

    #[inline] pub fn new() -> JsResult<Self> {
        Ok(Self{canvas: NodeRef::default(), selected_id: None,
            offset: Point::ZERO, redraw: true,
            focus: Focus::None, last_focus: discriminant(&Focus::HoverPlane),
            solid_line: JsArray::new().into(),
            dotted_line: JsArray::of2(&JsValue::from(10.0), &JsValue::from(10.0)).into(),
            scale_x: r64![20.0], scale_y: 10, snap_step: r64![1.0]})
    }

    #[inline] pub fn canvas(&self) -> &NodeRef {
        &self.canvas
    }
    
    #[inline] pub fn selected_element_id(&self) -> Option<usize> {
        self.selected_id
    }

    pub fn params(&self, hint: &Rc<HintHandler>) -> Html {
        html!{
            <div id="plane-settings" onpointerover={hint.setter("Editor plane settings", "")}>
                <Switch {hint} key="snap" name="Interval for blocks to snap to"
                    id={ParamId::SnapStep}
                    options={vec!["None", "1", "1/2", "1/4", "1/8"]}
                    initial={match *self.snap_step {
                        x if x == 1.0   => 1,
                        x if x == 0.5   => 2,
                        x if x == 0.25  => 3,
                        x if x == 0.125 => 4,
                        _ => 0
                    }}/>
            </div>
        }
    }

    /// the returned `bool` indicates whether the selected block's editor window should be
    /// rerendered
    pub fn set_param(&mut self, id: ParamId, value: R64) -> JsResult<bool> {
        if let ParamId::SnapStep = id {
            self.snap_step = *[r64![0.0], r64![1.0], r64![0.5], r64![0.25], r64![0.125]]
                .get_wrapping(*value as usize);
        }
        Ok(false)
    }

    pub fn handle_resize(&mut self) -> JsResult<()> {
        let canvas: HtmlCanvasElement = self.canvas().cast().to_js_result(loc!())?;
        let body = document().body().to_js_result(loc!())?;
        let [w, h] = body.client_size();
        let ctx = canvas.get_2d_context(loc!())?;
        canvas.set_width(w as u32);
        canvas.set_height(h as u32);
        ctx.set_line_width(3.0);
        self.redraw = true;
        Ok(())
    }

    #[inline] fn in_block(block: &PatternBlock, offset: Beats, layer: i32) -> bool {
        layer == block.layer
            && (*block.offset .. *block.sound.len()).contains(&*offset)
    }

    #[inline] fn id_by_pos(offset: Beats, layer: i32, pattern: &[PatternBlock]) -> Option<usize> {
        pattern.iter().position(|x| Self::in_block(x, offset, layer))
    }

    #[inline] fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        self.redraw = true;
    }

    pub fn set_event(&mut self, event: Option<CanvasEvent>, pattern: &mut [PatternBlock]) -> Option<(ParamId, R64)> {
        let Some(mut event) = event else {
            self.focus = Focus::None;
            self.last_focus = discriminant(&self.focus);
            return None
        };
        event.point += self.offset;
        let [w, h] = self.canvas.cast::<HtmlCanvasElement>()?.size();
        let offset = Beats::from(event.point.x) / w * self.scale_x;
        let layer = (event.point.x as f32 / h as f32 * self.scale_y as f32) as i32;

        match self.focus {
            Focus::None => self.set_focus(Focus::HoverPlane).pipe(|_| None),

            Focus::HoverPlane => match (event.left, Self::id_by_pos(offset, layer, pattern)) {
                (true, None) => Focus::MovePlane(event.point.x - self.offset.x),
                (true, Some(id)) => Focus::MoveElement(id, event.point),
                (false, None) => return None,
                (false, Some(id)) => Focus::HoverElement(id)
            }.pipe(|x| {self.set_focus(x); None}),

            Focus::HoverElement(id) => if event.left {
                Focus::MoveElement(id, event.point)
            } else if !Self::in_block(unsafe{pattern.get_unchecked(id)}, offset, layer) {
                Focus::HoverPlane
            } else {
                return None
            }.pipe(|x| {self.set_focus(x); None}),

            Focus::MovePlane(ref mut last_x) => if event.left {
                event.point.x -= self.offset.x;
                self.offset.x += *last_x - event.point.x;
                *last_x = event.point.x;
            } else {
                self.focus = Focus::HoverPlane;
            }.pipe(|_| {self.redraw = true; None}),

            Focus::MoveElement(id, ref mut point) => if event.left {
                let block = unsafe{pattern.get_unchecked_mut(id)};
                block.offset += Beats::from(event.point.x - point.x) / w * self.scale_x;
                block.layer = (block.layer as f32
                    + (event.point.y - point.y) as f32 / h as f32 * self.scale_y as f32)
                    as i32;
                *point = event.point;
                None
            } else {
                self.focus = Focus::HoverElement(id);
                self.selected_id = (self.selected_id != Some(id)).then_some(id);
                Some((ParamId::Select(self.selected_id), R64::INFINITY))
            }.tee(|_| self.redraw = true)
        }
    }

    pub fn poll(&mut self, pattern: &[PatternBlock], hint_handler: &HintHandler) -> JsResult<()> {
        if discriminant(&self.focus) != self.last_focus {
            self.last_focus = discriminant(&self.focus);
            match self.focus {
                Focus::None => (),
                Focus::HoverPlane =>
                    hint_handler.set_hint("Editor plane", "").add_loc(loc!())?,
                Focus::HoverElement(id) =>
                    hint_handler.set_hint(&pattern.get(id).to_js_result(loc!())?.name(),
                        "Press and hold to drag").add_loc(loc!())?,
                Focus::MovePlane(_) =>
                    hint_handler.set_hint("Editor plane", "Dragging").add_loc(loc!())?,
                Focus::MoveElement(id, _) =>
                    hint_handler.set_hint(&pattern.get(id).to_js_result(loc!())?.name(),
                        "Dragging").add_loc(loc!())?,
            }
        }

        if !self.redraw.take() {return Ok(())}

        let canvas: HtmlCanvasElement = self.canvas().cast().to_js_result(loc!())?;
        let [w, h] = canvas.size().map(|x| x as f64);
        let ctx = canvas.get_2d_context(loc!())?;

        ctx.set_fill_style(&Self::BG_STYLE.into());
        ctx.fill_rect(0.0, 0.0, w, h);
        ctx.set_fill_style(&Self::FG_STYLE.into());

        ctx.set_stroke_style(&Self::MG_STYLE.into());
        ctx.begin_path();
        let step = w / *self.scale_x;
        for mut i in succ(Some(0.0), |x| x.add(step).check_in(..w).ok()) {
            i -= self.offset.x as f64;
            ctx.move_to(i, 0.0);
            ctx.line_to(i, h);
        }
        Ok(ctx.stroke())
    }
}

#[derive(PartialEq, Default)]
pub struct HintHandler {
    main_bar: NodeRef,
    aux_bar: NodeRef
}

impl HintHandler {
    const DEFAULT_MAIN: &str = "Here will be a hint";
    const DEFAULT_AUX: &str = "Hover over anything to get a hint";

    #[inline] pub fn set_hint(&self, main: &str, aux: &str) -> JsResult<()> {
        self.main_bar.cast::<HtmlElement>().to_js_result(loc!())?
            .set_inner_text(main);
        self.aux_bar.cast::<HtmlElement>().to_js_result(loc!())?
            .set_inner_text(aux);
        Ok(())
    }

    #[inline] pub fn clear_hint(&self) -> JsResult<()> {
        self.main_bar.cast::<HtmlElement>().to_js_result(loc!())?
            .set_inner_text(Self::DEFAULT_MAIN);
        self.aux_bar.cast::<HtmlElement>().to_js_result(loc!())?
            .set_inner_text(Self::DEFAULT_AUX);
        Ok(())
    }

    pub fn setter<T>(self: &Rc<Self>, main: impl AsRef<str>, aux: impl AsRef<str>)
    -> impl Fn(T) {
        let res = Rc::clone(self);
        move |_| _ = res.set_hint(main.as_ref(), aux.as_ref()).report_err(loc!())
    }

    #[inline] pub fn main_bar(&self) -> &NodeRef {
        &self.main_bar
    }

    #[inline] pub fn aux_bar(&self) -> &NodeRef {
        &self.aux_bar
    }
}
