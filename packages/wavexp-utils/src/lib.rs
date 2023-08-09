#![feature(const_float_classify)]
#![feature(never_type)]
#![feature(try_blocks)]

use std::{
    ptr,
    mem::{take, transmute_copy, MaybeUninit, forget},
    fmt::{self, Debug, Formatter, Display},
    ops::{Neg, SubAssign, Sub, AddAssign, Add, Deref, RangeBounds, Mul, MulAssign, DivAssign, Div, Rem, RemAssign, Range},
    iter::{successors, Sum},
    error::Error,
    collections::TryReserveError,
    cmp::Ordering,
    any::type_name,
    array::from_fn,
    num::{TryFromIntError,
        NonZeroU8, NonZeroU16, NonZeroU32, NonZeroUsize, NonZeroU64,
        NonZeroI8, NonZeroI16, NonZeroI32, NonZeroIsize, NonZeroI64}, backtrace::Backtrace, borrow::Cow
};
pub use js_sys;
pub use wasm_bindgen;
use wasm_bindgen::{JsValue, JsCast};
use web_sys::{
    Document as HtmlDocument,
    Window as HtmlWindow,
    CanvasRenderingContext2d,
    HtmlCanvasElement,
    Element,
    console::warn_1,
    HtmlElement};
use yew::html::IntoPropValue;

pub struct EveryNth<'a, T> {
    iter: &'a [T],
    n: usize,
    state: usize,
    off: usize
}

impl<'a, T> Iterator for EveryNth<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		if let res @Some(_) = self.iter.get(self.state) {
            self.state += self.n;
            res
        } else {
            self.off += 1;
            if self.off == self.n {
                None
            } else {
                self.state = self.off + self.n;
                self.iter.get(self.state - self.n)
            }
        }
    }
}

pub struct EveryNthMut<'a, T> {
    iter: &'a mut [T],
    n: usize,
    state: usize,
    off: usize
}

impl<'a, T> Iterator for EveryNthMut<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		if let Some(res) = self.iter.get_mut(self.state) {
            self.state += self.n;
            Some(unsafe{(res as *mut T).as_mut().unwrap_unchecked()})
        } else {
            self.off += 1;
            if self.off == self.n {
                None
            } else {
                self.state = self.off + self.n;
                self.iter.get_mut(self.state - self.n)
                    .map(|x| unsafe{(x as *mut T).as_mut().unwrap_unchecked()})
            }
        }
    }
}

pub trait ToEveryNth<T> {
    fn every_nth(&self, n: usize) -> EveryNth<'_, T>;
    fn every_nth_mut(&mut self, n: usize) -> EveryNthMut<'_, T>;
}

impl<T> ToEveryNth<T> for [T] {
    #[inline] fn every_nth(&self, n: usize) -> EveryNth<'_, T> {
        EveryNth {iter: self, n, state: 0, off: 0}
    }
    #[inline] fn every_nth_mut(&mut self, n: usize) -> EveryNthMut<'_, T> {
        EveryNthMut {iter: self, n, state: 0, off: 0}
    }
}

#[test]
fn test_every_nth_mut() {
    let mut data = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let transposed: Vec<u8>     = data.every_nth(3).copied().collect();
    let transposed_mut: Vec<u8> = data.every_nth_mut(3).map(|x| *x).collect();
    assert_eq!(transposed,     [0, 3, 6, 9, 1, 4, 7, 10, 2, 5, 8]);
    assert_eq!(transposed_mut, [0, 3, 6, 9, 1, 4, 7, 10, 2, 5, 8]);
}

pub struct IterIndicesMut<'data, 'ids, T> {
    data: &'data mut [T],
    /// all indices are valid, trust me bro
    ids: &'ids [usize],
    state: usize
}

impl<'data, 'ids, T> Iterator for IterIndicesMut<'data, 'ids, T> {
    type Item = &'data mut T;
    #[inline] fn next(&mut self) -> Option<Self::Item> {
        if let Some(&id) = self.ids.get(self.state) {
            self.state += 1;
            unsafe {
                (self.data.get_unchecked_mut(id) as *mut T).as_mut()
            }
        } else {None}
    }
}

pub trait ToIterIndicesMut<'data, 'ids, T> {
    unsafe fn iter_indices_unchecked_mut(&'data mut self, ids: &'ids [usize])
    -> IterIndicesMut<'data, 'ids, T>;
    fn iter_indices_mut(&'data mut self, ids: &'ids [usize])
    -> Option<IterIndicesMut<'data, 'ids, T>>;
}

impl<'data, 'ids, T> ToIterIndicesMut<'data, 'ids, T> for [T] {
    #[inline] unsafe fn iter_indices_unchecked_mut(&'data mut self, ids: &'ids [usize])
    -> IterIndicesMut<'data, 'ids, T> {
        IterIndicesMut{data: self, ids, state: 0}
    }
    
    #[inline] fn iter_indices_mut(&'data mut self, ids: &'ids [usize])
    -> Option<IterIndicesMut<'data, 'ids, T>> {
        let len = self.len();
        if ids.iter().any(|&i| i >= len) {return None}
        Some(IterIndicesMut{data: self, ids, state: 0})
    }
}

pub fn modify<T>(src: &mut T, f: impl FnOnce(T) -> T) {
    let src = src as *mut T;
    unsafe{src.write(f(src.read()))}
}

pub fn default<T: Default>() -> T {T::default()}

#[repr(transparent)]
pub struct Alias<'a, T: ?Sized>(pub &'a T);

impl<'a, T: ?Sized + Deref> Deref for Alias<'a, T> {
    type Target = T::Target;
    #[inline] fn deref(&self) -> &Self::Target {self.0.deref()}
}

pub trait Check: Sized {
	#[inline] fn check(self, f: impl FnOnce(&Self) -> bool) -> Result<Self, Self> {
		if f(&self) {Ok(self)} else {Err(self)}
	}

    #[inline] fn check_in<R>(self, range: R) -> Result<Self, Self>
	where Self: PartialOrd, R: RangeBounds<Self> {
		if range.contains(&self) {Ok(self)} else {Err(self)}
	}

	#[inline] fn check_not_in<R>(self, range: R) -> Result<Self, Self>
	where Self: PartialOrd, R: RangeBounds<Self> {
		if !range.contains(&self) {Ok(self)} else {Err(self)}
	}
}
impl<T> Check for T {}

pub trait Tee: Sized {
	#[inline] fn tee(self, f: impl FnOnce(&Self)) -> Self {
        f(&self); self
	}

    #[inline] fn js_log(self, label: &str) -> Self 
    where Self: Debug {
        js_log!("{}{:?}", label, &self); self
    }
}
impl<T> Tee for T {}

pub trait Pipe: Sized {
	#[inline] fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }

    #[inline] fn pipe_if(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond {f(self)} else {self}
    }
}
impl<T> Pipe for T {}

pub trait BoolExt {
	fn choose<T>(self, on_true: T, on_false: T) -> T;
    fn then_or<T>(self, default: T, f: impl FnOnce() -> T) -> T;
    fn then_or_else<T>(self, default: impl FnOnce() -> T, f: impl FnOnce() -> T) -> T;
    fn then_negate<T: Neg<Output=T>>(self, val: T) -> T;
    fn then_try<T, E>(self, f: impl FnOnce() -> Result<T, E>) -> Result<Option<T>, E>;
    fn and_then<T>(self, f: impl FnOnce() -> Option<T>) -> Option<T>;
    fn report_false(self) -> Option<()>;
    fn to_js_result(self) -> JsResult<()>;
}

impl BoolExt for bool {
    #[inline] fn choose<T>(self, on_true: T, on_false: T) -> T {
        if self {on_true} else {on_false}
    }

    #[inline] fn then_or<T>(self, default: T, f: impl FnOnce() -> T) -> T {
        if self {f()} else {default}
    }

    #[inline] fn then_or_else<T>(self, default: impl FnOnce() -> T, f: impl FnOnce() -> T) -> T {
        if self {f()} else {default()}
    }

    #[inline] fn then_negate<T: Neg<Output=T>>(self, val: T) -> T {
        if self {-val} else {val}
    }

    #[inline] fn then_try<T, E>(self, f: impl FnOnce() -> Result<T, E>) -> Result<Option<T>, E> {
        self.then(f).transpose()
    }

    #[inline] fn and_then<T>(self, f: impl FnOnce() -> Option<T>) -> Option<T> {
        if self {f()} else {None}
    }

    #[inline] fn report_false(self) -> Option<()> {
        self.to_js_result().report()
    }

    #[inline] fn to_js_result(self) -> JsResult<()> {
        if self {Ok(())}
        else {Err(js_sys::Error::new("expected `true`, found `false`").into())}
    }
}

pub trait ArrayExt<T, const N: usize>: Sized {
    fn zip<O, R>(self, other: [O; N], f: impl FnMut(T, O) -> R) -> [R; N];
    fn zip_fold<O, R>(self, init: R, other: [O; N], f: impl FnMut(R, T, O) -> R) -> R;
    fn add<'a, O>(self, other: &'a [O; N]) -> Self
    where T: AddAssign<&'a O>;
    fn sub<'a, O>(self, other: &'a [O; N]) -> Self
    where T: SubAssign<&'a O>;
    fn mul<'a, O>(self, other: &'a [O; N]) -> Self
    where T: MulAssign<&'a O>;
    fn div<'a, O>(self, other: &'a [O; N]) -> Self
    where T: DivAssign<&'a O>;
    fn rem<'a, O>(self, other: &'a [O; N]) -> Self
    where T: RemAssign<&'a O>;
    fn floor_to(self, other: Self) -> Self
    where T: RoundTo;
    fn ceil_to(self, other: Self) -> Self
    where T: RoundTo;
    fn sum<R>(self) -> R
    where R: Sum<T>;
    fn array_check_in<R, O>(self, ranges: &[R; N]) -> Option<Self>
    where T: PartialOrd<O>, O: PartialOrd<T>, R: RangeBounds<O>;
    fn fit<R, O>(&self, values: [R; N]) -> [R; N]
    where T: RangeExt<O>, O: Clone + PartialOrd<R>, R: Clone + From<O>;
}

impl<T, const N: usize> ArrayExt<T, N> for [T; N] {
    #[inline] fn zip<O, R>(self, other: [O; N], mut f: impl FnMut(T, O) -> R) -> [R; N] {
        let (mut d, mut s) = (self.into_iter(), other.into_iter());
        from_fn(|_| unsafe{f(d.next().unwrap_unchecked(), s.next().unwrap_unchecked())})
    }

    #[inline] fn zip_fold<O, R>(self, init: R, other: [O; N], mut f: impl FnMut(R, T, O) -> R) -> R {
        self.into_iter().zip(other).fold(init, |r, (x, y)| f(r, x, y))
    }

    #[inline] fn add<'a, O>(mut self, other: &'a [O; N]) -> Self where T: AddAssign<&'a O> {
        for (dst, src) in self.iter_mut().zip(other.iter()) {*dst += src}
        self
    }

    #[inline] fn sub<'a, O>(mut self, other: &'a [O; N]) -> Self where T: SubAssign<&'a O> {
        for (dst, src) in self.iter_mut().zip(other.iter()) {*dst -= src}
        self
    }

    #[inline] fn mul<'a, O>(mut self, other: &'a [O; N]) -> Self where T: MulAssign<&'a O> {
        for (dst, src) in self.iter_mut().zip(other.iter()) {*dst *= src}
        self
    }

    #[inline] fn div<'a, O>(mut self, other: &'a [O; N]) -> Self where T: DivAssign<&'a O> {
        for (d, s) in self.iter_mut().zip(other.iter()) {*d /= s}
        self
    }

    #[inline] fn rem<'a, O>(mut self, other: &'a [O; N]) -> Self where T: RemAssign<&'a O> {
        for (d, s) in self.iter_mut().zip(other.iter()) {*d %= s}
        self
    }

    #[inline] fn floor_to(mut self, other: Self) -> Self where T: RoundTo {
        for (d, s) in self.iter_mut().zip(other) {modify(d, |d| d.floor_to(s))}
        self
    }

    #[inline] fn ceil_to(mut self, other: Self) -> Self where T: RoundTo {
        for (d, s) in self.iter_mut().zip(other) {modify(d, |d| d.ceil_to(s))}
        self
    }

    #[inline] fn sum<R>(self) -> R where R: Sum<T> {self.into_iter().sum()}

    #[inline] fn array_check_in<R, O>(self, ranges: &[R; N]) -> Option<Self>
    where T: PartialOrd<O>, O: PartialOrd<T>, R: RangeBounds<O> {
        self.iter().zip(ranges).all(|(i, r)| r.contains(i)).then_some(self)
    }

    #[inline] fn fit<R, O>(&self, mut values: [R; N]) -> [R; N]
    where T: RangeExt<O>, O: Clone + PartialOrd<R>, R: Clone + From<O> {
        for (i, r) in values.iter_mut().zip(self) {
            *i = r.fit(i.clone());
        }
        values
    }
}

pub trait ArrayFrom<T, const N: usize>: Sized {
    fn array_from(x: T) -> [Self; N];
}

impl<S, D, const N: usize> ArrayFrom<[S; N], N> for D where D: From<S> {
    #[inline] fn array_from(x: [S; N]) -> [Self; N] {x.map(D::from)}
}

pub trait IntoArray<T, const N: usize> {
    fn into_array(self) -> [T; N];
}

impl<T, U, const N: usize> IntoArray<U, N> for T where U: ArrayFrom<T, N> {
    #[inline] fn into_array(self) -> [U; N] {U::array_from(self)}
}

pub trait FlippedArray<T, const OUTER: usize, const INNER: usize> {
    fn flipped(self) -> [[T; OUTER]; INNER];
}

impl<T, const OUTER: usize, const INNER: usize> FlippedArray<T, OUTER, INNER> for [[T; INNER]; OUTER] {
    fn flipped(mut self) -> [[T; OUTER]; INNER] {
        unsafe {
            if OUTER == INNER {
                let mut src = self.as_mut_ptr() as *mut T;
                for outer in 0 .. OUTER {
                    src = src.add(outer + 1);
                    for inner in outer + 1 .. INNER {
                        src.swap(src.add((inner - outer) * (INNER - 1)));
                        src = src.add(1);
                    }
                }
                transmute_copy(&self)
            } else {
                let mut new_self: MaybeUninit<_> = MaybeUninit::uninit();
                let mut res = new_self.as_mut_ptr() as *mut T;
                for inner in 0 .. INNER {
                    let mut src = (self.as_mut_ptr() as *mut T).add(inner);
                    for _ in 0 .. OUTER {
                        res.copy_from(src, 1);
                        res = res.add(1);
                        src = src.add(INNER);
                    }
                }
                forget(self);
                new_self.assume_init()
            }
        }
    }
}

/// this exists to circumvent a limiatation on static variables that Rust imposes, which prevents
/// them from containing types that don't implement `Sync`. On any other architecture this
/// limitation makes sense, but in Webassembly, which doesn't support threading, this limitation is meaningless.
pub struct WasmCell<T>(T);

unsafe impl<T> Sync for WasmCell<T> {}

impl<T> Deref for WasmCell<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {&self.0}
}

impl<T> WasmCell<T> {
    pub const fn new(val: T) -> Self {Self(val)}
}

pub struct SliceRef<'a, T: ?Sized> {
    inner: &'a T,
    index: usize
}

impl<'a, T> Deref for SliceRef<'a, T> {
    type Target = T;
    #[inline] fn deref(&self) -> &Self::Target {self.inner}
}

impl<'a, T> SliceRef<'a, T> {
    #[inline] pub fn new(slice: &'a [T], index: usize) -> Option<Self> {
        slice.get(index).map(|inner| Self{inner, index}) 
    }

    #[inline] pub unsafe fn raw(inner: &'a T, index: usize) -> Self {Self{inner, index}}

    #[inline] pub fn index(&self) -> usize {self.index}
}

#[allow(non_camel_case_types)]
#[allow(dead_code)]
pub mod js_types {
    use js_sys::{Number as JsNumber, JsString, Boolean as JsBoolean};
	pub type bool = JsBoolean;
	pub type number = JsNumber;
	pub type str = JsString;
}

#[macro_export]
macro_rules! js_array {
    ($($t:ident $v:expr),*) => {{
        let res = $crate::js_sys::Array::new();
        $( res.push(&*$crate::js_types::$t::from($v)); )*
        $crate::wasm_bindgen::JsValue::from(res)
    }};
}

#[macro_export]
macro_rules! js_obj {
	($($t:ident $k:ident : $v:expr),*) => {
		$crate::wasm_bindgen::JsValue::from($crate::js_sys::Map::new()
			$( .set(&$crate::js_types::str::from(stringify!($k)).into(),
				&*$crate::js_types::$t::from($v)) )*)
	}
}

pub use web_sys::console::log_1;
#[macro_export]
macro_rules! js_log {
	($arg:literal) => {
        $crate::log_1(&format!($arg).into())
	};
	($f:literal, $($arg:expr),*) => {
		$crate::log_1(&format!($f, $($arg),*).into())
	}
}

#[macro_export]
macro_rules! js_try {
    (type = $r:ty : $($s:tt)*) => {
        {let x: $crate::JsResult<$r> = try {
            $($s)*
        }; x}
    };

    ($($s:tt)*) => {
        {let x: $crate::JsResult<_> = try {
            $($s)*
        }; x}
    };
}

#[macro_export]
macro_rules! js_assert {
    ($($s:tt)+) => {
        if !$($s)+ {
            Err($crate::wasm_bindgen::JsValue::from($crate::js_sys::Error::new(stringify!($($s)+))))
        } else {
            Ok(())
        }
    };
}

#[macro_export]
macro_rules! eval_once {
    ($t:ty : $e:expr) => {{
        static RES: $crate::WasmCell<std::cell::OnceCell<$t>> = $crate::WasmCell::new(std::cell::OnceCell::new());
        RES.get_or_init(|| $e)
    }};
}

pub fn window() -> HtmlWindow {
	unsafe{web_sys::window().unwrap_unchecked()}
}

pub fn document() -> HtmlDocument {
	unsafe{web_sys::window().unwrap_unchecked().document().unwrap_unchecked()}
}

pub fn report_err(err: JsValue) {
    warn_1(&to_error_with_msg(err, &format!("{}", Backtrace::capture())));
    if let Some(x) = document().element_dyn_into::<HtmlElement>("error-sign") {
        x.set_hidden(false)
    } else {
        warn_1(&JsValue::from("#error-sign element not found in the DOM"))
    }
}

#[inline] fn to_error_with_msg(err: JsValue, msg: &str) -> JsValue {
    let s = format!("{}\n{}", msg, 
        match err.dyn_into::<js_sys::Error>() {
            Ok(val) => val.message(),
            Err(val) => js_sys::Object::from(val).to_string()});
    js_sys::Error::new(&s).into()
}

pub type JsResult<T> = Result<T, JsValue>;

pub trait ResultExt<T, E> {
    fn to_js_result(self) -> JsResult<T> where E: Display;
    fn report_fmt(self) -> Option<T> where E: Display;
    fn report_as(self, msg: &str) -> Option<T>;
    fn report_with(self, f: impl FnOnce() -> Cow<'static, str>) -> Option<T>;
}

pub trait JsResultUtils<T>: Sized {
	fn report(self) -> Option<T>;
}

impl<T> JsResultUtils<T> for JsResult<T> {
    #[inline] fn report(self) -> Option<T> {
        match self {
            Ok(x) => Some(x),
            Err(e) => {report_err(e); None}
        }
    }
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    #[inline] fn to_js_result(self) -> JsResult<T> where E: Display {
        self.map_err(|e| e.to_string().into())
    }

    #[inline] fn report_fmt(self) -> Option<T> where E: Display {
        match self {
            Ok(x) => Some(x),
            Err(e) => {report_err(js_sys::Error::new(&e.to_string()).into()); None}
        }
    }

    #[inline] fn report_as(self, msg: &str) -> Option<T> {
        match self {
            Ok(x) => Some(x),
            Err(_) => {report_err(js_sys::Error::new(msg).into()); None}
        }
    }

    #[inline] fn report_with(self, f: impl FnOnce() -> Cow<'static, str>) -> Option<T> {
        match self {
            Ok(x) => Some(x),
            Err(_) => {report_err(js_sys::Error::new(&f()).into()); None}
        }
    }
}

pub trait OptionExt<T> {
    fn to_js_result(self) -> JsResult<T>;
    fn report_none(self) -> Self;
    fn map_or_default<U: Default>(self, f: impl FnOnce(T) -> U) -> U;
    fn choose<U>(&self, on_some: U, on_none: U) -> U;
    fn drop(self) -> Option<()>;
    fn get_or_try_insert<E>(&mut self, f: impl FnOnce() -> Result<T, E>) -> Result<&mut T, E>;
}

impl<T> OptionExt<T> for Option<T> {
    #[inline] fn to_js_result(self) -> JsResult<T> {
        self.ok_or_else(|| js_sys::Error::new("`Option` contained the `None` value").into())
    }

    #[inline] fn report_none(self) -> Self {
        if self.is_none() {
            report_err(js_sys::Error::new("`Option` contained the `None` value").into())
        }
        self
    }

    #[inline] fn map_or_default<U: Default>(self, f: impl FnOnce(T) -> U) -> U {
        match self {Some(x) => f(x), None => U::default()}
    }

    #[inline] fn choose<U>(&self, on_some: U, on_none: U) -> U {
        if self.is_some() {on_some} else {on_none}
    }

    #[allow(clippy::manual_map)]
    #[inline] fn drop(self) -> Option<()> {
        match self {Some(_) => Some(()), None => None}
    }

    #[inline] fn get_or_try_insert<E>(&mut self, f: impl FnOnce() -> Result<T, E>) -> Result<&mut T, E> {
        if self.is_none() {
            *self = Some(f()?);
        }
        Ok(unsafe{self.as_mut().unwrap_unchecked()})
    }
}

pub trait HtmlCanvasExt {
    fn get_2d_context(&self) -> Option<CanvasRenderingContext2d>;
    fn rect(&self) -> Rect;
    fn size(&self) -> [u32; 2];
    fn sync(&self);
}

impl HtmlCanvasExt for HtmlCanvasElement {
    fn get_2d_context(&self) -> Option<CanvasRenderingContext2d> {
        Some(self.get_context("2d").report()?.report_none()?.unchecked_into())
    }

    fn rect(&self) -> Rect {
        Rect(Point::ZERO, Point{x: self.width() as i32, y: self.height() as i32})
    }

    fn size(&self) -> [u32; 2] {[self.width(), self.height()]}

    fn sync(&self) {
        self.set_height((self.client_height() as f64 / self.client_width() as f64 * self.width() as f64) as u32);
    }
}

pub trait HtmlDocumentExt {
    fn element_dyn_into<T: JsCast>(&self, id: &str) -> Option<T>;
}

impl HtmlDocumentExt for HtmlDocument {
    fn element_dyn_into<T: JsCast>(&self, id: &str) -> Option<T> {
        self.get_element_by_id(id).to_js_result().report()?
            .dyn_into::<T>().report_with(|| format!("#{id} is not of type {}", type_name::<T>()).into())
    }
}

pub trait HtmlElementExt {
    fn client_rect(&self) -> Rect;
    fn client_size(&self) -> [i32; 2];
}

impl HtmlElementExt for Element {
    fn client_rect(&self) -> Rect {
        Rect(Point::ZERO, Point{x: self.client_width(), y: self.client_height()})
    }

    fn client_size(&self) -> [i32; 2] {
        [self.client_width(), self.client_height()]
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum GetVarError {
    OutOfBounds(usize, usize),
    Overlap(usize)
}

impl Display for GetVarError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GetVarError::OutOfBounds(x, len) =>
                write!(f, "index #{x} is out of bounds for a slice of length {len}"),
            GetVarError::Overlap(x) =>
                write!(f, "index #{x} appeared more than once")}
    }
}

impl Error for GetVarError {}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ReorderError {
    index: usize,
    len: usize
}

impl Display for ReorderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "SliceExt::reorder: index #{} is out of bounds for a slice of length {}",
            self.index, self.len)
    }
}

impl Error for ReorderError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceMove {pub from: usize, pub to: usize}

impl SliceMove {
    pub fn apply(&self, ids: &mut [usize]) {
        let (coef, range) = match self.to.cmp(&self.from) {
            Ordering::Less => (1, self.to .. self.from),
            Ordering::Equal => return,
            Ordering::Greater => (-1, self.from .. self.to)
        };

        for id in ids {
            if *id == self.from {
                *id = self.to;
            } else if range.contains(id) {
                // not going to wrap anyway
                *id = id.wrapping_add_signed(coef);
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct SetSortedError {
    index: usize,
    len: usize
}

impl Display for SetSortedError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "SliceExt::set_sorted: index #{} is out of bounds for a slice of length {}",
            self.index, self.len)
    }
}

impl Error for SetSortedError {}

pub struct IterMutWithCtx<'a, T: 'a + Copy> {
    slice: &'a mut [T],
    state: usize
}

impl<'a, T: 'a + Copy> Iterator for IterMutWithCtx<'a, T> {
    type Item = (&'a mut [T], T);
    #[inline] fn next(&mut self) -> Option<Self::Item> {
        self.slice.get(self.state).copied().map(|x| unsafe {
            self.state += 1;
            ((self.slice as *mut [T]).as_mut().unwrap_unchecked(), x)})
    }

    #[inline] fn size_hint(&self) -> (usize, Option<usize>) {
        self.slice.len().pipe(|x| (x, Some(x)))
    }
}

impl<'a, T: 'a + Copy> ExactSizeIterator for IterMutWithCtx<'a, T> {
    #[inline] fn len(&self) -> usize {self.slice.len()}
}

impl<'a, T: 'a + Copy> IterMutWithCtx<'a, T> {
    #[inline] fn new(slice: &'a mut [T]) -> Self {Self{slice, state: 0}}
}

pub trait SliceExt<T> {
    fn any(&self, f: impl FnMut(&T) -> bool) -> bool;
    fn all(&self, f: impl FnMut(&T) -> bool) -> bool;
    fn to_box(&self) -> Box<Self> where T: Clone;
    fn get_saturating(&self, id: usize) -> &T;
    fn get_saturating_mut(&mut self, id: usize) -> &mut T;
    fn get_wrapping(&self, id: usize) -> &T;
    fn get_wrapping_mut(&mut self, id: usize) -> &mut T;
    fn get_var<'a>(&'a self, ids: &[usize]) -> Result<Vec<&'a T>, GetVarError>;
    fn get_var_mut<'a>(&'a mut self, ids: &[usize]) -> Result<Vec<&'a mut T>, GetVarError>;
    unsafe fn reorder_unchecked(&mut self, index: usize) -> SliceMove
        where T: Ord;
    // unsafe fn reorder_unchecked_by<F>(&mut self, index: usize, f: F) -> usize
    //  where F: FnMut(&T, &T) -> Ordering
    unsafe fn reorder_unchecked_by_key<K, F>(&mut self, index: usize, f: F) -> SliceMove
        where F: FnMut(&T) -> K, K: Ord;
    fn reorder(&mut self, index: usize) -> Result<SliceMove, ReorderError>
        where T: Ord;
    // fn reorder_by<F>(&mut self, index: usize, f: F) -> Result<usize, ReorderError>
    //  where F: FnMut(&T, &T) -> Ordering
    // fn reorder_by_key<K, F>(&mut self, index: usize, f: F) -> Result<usize, ReorderError>
    //  where F: FnMut(&T) -> K, K: Ord
    fn set_sorted(&mut self, index: usize, value: T) -> Result<SliceMove, SetSortedError>
        where T: Ord;
    // fn set_sorted_by<F>(&mut self, index: usize, value: T, f: F) -> Result<usize, SetSortedError>
    //  where F: FnMut(&T, &T) -> Ordering
    // fn set_sorted_by_key<K, F>(&mut self, index: usize, value: T, f: F) -> Result<usize, SetSortedError>
    //  where F: FnMut(&T) -> K, K: Ord
    fn get_aware(&self, index: usize) -> Option<SliceRef<'_, T>>;
    unsafe fn get_unchecked_aware(&self, index: usize) -> SliceRef<'_, T>;
    fn iter_mut_with_ctx<'a>(&'a mut self) -> IterMutWithCtx<'a, T> where T: 'a + Copy;
}

impl<T> SliceExt<T> for [T] {
    #[inline] fn any(&self, mut f: impl FnMut(&T) -> bool) -> bool {
        let mut res = false;
        for i in self {res |= f(i)}
        res
    }

    #[inline] fn all(&self, mut f: impl FnMut(&T) -> bool) -> bool {
        let mut res = true;
        for i in self {res &= f(i)}
        res
    }

    #[inline] fn to_box(&self) -> Box<Self> where T: Clone {self.into()}

    #[inline] fn get_saturating(&self, id: usize) -> &T {
        unsafe{self.get_unchecked(id.min(self.len() - 1))}
    }

    #[inline] fn get_saturating_mut(&mut self, id: usize) -> &mut T {
        unsafe{self.get_unchecked_mut(id.min(self.len() - 1))}
    }

    #[inline] fn get_wrapping(&self, id: usize) -> &T {
        unsafe{self.get_unchecked(id % self.len())}
    }

    #[inline] fn get_wrapping_mut(&mut self, id: usize) -> &mut T {
        unsafe{self.get_unchecked_mut(id % self.len())}
    }


    #[inline] fn get_var<'a>(&'a self, ids: &[usize]) -> Result<Vec<&'a T>, GetVarError> {
        let len = self.len();
        for (id, rest) in successors(ids.split_first(), |x| x.1.split_first()) {
            if *id >= len {return Err(GetVarError::OutOfBounds(*id, len))}
            if rest.contains(id) {return Err(GetVarError::Overlap(*id))}
        }
        Ok(unsafe { // at this point, `ids` is guaranteed to contain unique valid indices into `self`
            let base = self.as_ptr();
            ids.iter().map(|x| &*base.add(*x)).collect::<Vec<_>>()
        })
    }

    #[inline] fn get_var_mut<'a>(&'a mut self, ids: &[usize]) -> Result<Vec<&'a mut T>, GetVarError> {
        let len = self.len();
        for (id, rest) in successors(ids.split_first(), |x| x.1.split_first()) {
            if *id >= len {return Err(GetVarError::OutOfBounds(*id, len))}
            if rest.contains(id) {return Err(GetVarError::Overlap(*id))}
        }
        Ok(unsafe { // at this point, `ids` is guaranteed to contain unique valid indices into `self`
            let base = self.as_mut_ptr();
            ids.iter().map(|x| &mut*base.add(*x)).collect::<Vec<_>>()
        })
    }

    unsafe fn reorder_unchecked(&mut self, index: usize) -> SliceMove where T: Ord {
        let element = self.get_unchecked(index);
        let (new, should_move) = self.get_unchecked(..index).binary_search(element)
            .map_or_else(|x| (x, x != index), |x| (x, x < index - 1));
        if should_move {
            self.get_unchecked_mut(new..=index).rotate_right(1);
            return SliceMove{from: index, to: new}
        }
        let new = self.get_unchecked(index+1..).binary_search(element)
            .unwrap_or_else(|x| x) + index;
        if new > index {
            self.get_unchecked_mut(index..=new).rotate_left(1);
        }
        SliceMove{from: index, to: new}
    }

    unsafe fn reorder_unchecked_by_key<K, F>(&mut self, index: usize, mut f: F) -> SliceMove
    where F: FnMut(&T) -> K, K: Ord {
        let key = f(self.get_unchecked(index));
        let (new, should_move) = self.get_unchecked(..index).binary_search_by_key(&key, &mut f)
            .map_or_else(|x| (x, x != index), |x| (x, x < index - 1));
        if should_move {
            self.get_unchecked_mut(new..=index).rotate_right(1);
            return SliceMove{from: index, to: new}}
        let new = self.get_unchecked(index+1..).binary_search_by_key(&key, &mut f)
            .unwrap_or_else(|x| x) + index;
        if new > index {
            self.get_unchecked_mut(index..=new).rotate_left(1);
        }
        SliceMove{from: index, to: new}
    }

    #[inline] fn reorder(&mut self, index: usize) -> Result<SliceMove, ReorderError> where T: Ord {
        let len = self.len();
        if index >= len {
            return Err(ReorderError{index, len});
        }
        Ok(unsafe{self.reorder_unchecked(index)})
    }

    #[inline] fn set_sorted(&mut self, index: usize, value: T) -> Result<SliceMove, SetSortedError> where T: Ord {
        let len = self.len();
        if index >= len {
            return Err(SetSortedError{index, len});
        }
        Ok(unsafe {
            let dst = self.get_unchecked_mut(index);
            let should_reorder = &value != dst;
            *dst = value;
            if should_reorder {self.reorder_unchecked(index)} else {SliceMove{from: index, to: index}}
        })
    }

    #[inline] fn get_aware(&self, index: usize) -> Option<SliceRef<'_, T>> {SliceRef::new(self, index)}

    #[inline] unsafe fn get_unchecked_aware(&self, index: usize) -> SliceRef<'_, T> {
        SliceRef::raw(self.get_unchecked(index), index)
    }

    #[inline] fn iter_mut_with_ctx<'a>(&'a mut self) -> IterMutWithCtx<'a, T> where T: 'a + Copy {
        IterMutWithCtx::new(self)
    }
}

#[test] fn slice_get_var() {
    let x = [1, 2, 4, 8, 16, 32, 64];
    assert_eq!(x.get_var(&[1, 3, 6]), Ok(vec![&2, &8, &64]));
    assert_eq!(x.get_var(&[1, 25]), Err(GetVarError::OutOfBounds(25, 7)));
    assert_eq!(x.get_var(&[1, 4, 5, 1]), Err(GetVarError::Overlap(1)));
}

#[test] fn slice_get_var_mut() {
    let mut x = [1, 2, 4, 8, 16, 32, 64];
    assert_eq!(x.get_var_mut(&[1, 3, 6]), Ok(vec![&mut 2, &mut 8, &mut 64]));
    assert_eq!(x.get_var_mut(&[1, 25]), Err(GetVarError::OutOfBounds(25, 7)));
    assert_eq!(x.get_var_mut(&[1, 4, 5, 1]), Err(GetVarError::Overlap(1)));
}

#[test] fn slice_reorder() {
    let mut x = [1, 2, 4, 8, 16, 32, 64];
    let old_x = x;
    assert_eq!(x.reorder(3), Ok(SliceMove{from: 3, to: 3}));
    assert_eq!(x, old_x);
    x[1] = 17;
    assert_eq!(x.reorder(1), Ok(SliceMove{from: 1, to: 4}));
    // [1, 2, 4, 8, 16, 32, 64] > [1, 4, 8, 16, 17, 32, 64]
    x[5] = 3;
    assert_eq!(x.reorder(5), Ok(SliceMove{from: 5, to: 1}));
    // [1, 4, 8, 16, 17, 32, 64] > [1, 3, 4, 8, 16, 17, 64]
    let old_x = x;
    assert_eq!(x.reorder(69), Err(ReorderError{index: 69, len: 7}));
    assert_eq!(x, old_x);
    x[2] = 3;
    assert_eq!(x.reorder(2), Ok(SliceMove{from: 2, to: 2}));
}

#[derive(Debug, PartialEq, Eq)]
pub struct RemoveError {
    index: usize,
    len: usize
}

impl Display for RemoveError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "removal index (is {}) should be < len (is {})", self.index, self.len)
    }
}

impl Error for RemoveError {}

#[derive(Debug)]
pub enum InsertError {
    Index{index: usize, len: usize},
    Alloc(TryReserveError)
}

impl Display for InsertError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Index{index, len} => write!(f, "insertion index (is {}) should be <= len (is {})", index, len),
            Self::Alloc(err) => Debug::fmt(err, f)
        }
    }
}

impl Error for InsertError {}

pub trait VecExt<T> {
    fn try_remove(&mut self, index: usize) -> Result<T, RemoveError>;
    unsafe fn remove_unchecked(&mut self, index: usize) -> T;
    fn try_swap_remove(&mut self, index: usize) -> Result<T, RemoveError>;
    fn try_insert(&mut self, index: usize, element: T) -> Result<&mut T, InsertError>;
    fn push_unique(&mut self, value: T, f: impl Fn(&T, &T) -> bool) -> bool;
    fn push_sorted(&mut self, value: T) -> usize where T: Ord;
    fn push_sorted_by(&mut self, value: T, f: impl Fn(&T, &T) -> Ordering) -> usize;
    fn push_sorted_by_key<K: Ord>(&mut self, value: T, f: impl FnMut(&T) -> K) -> usize;
}

impl<T> VecExt<T> for Vec<T> {
    #[inline] fn try_remove(&mut self, index: usize) -> Result<T, RemoveError> {
        let len = self.len();
        if index >= len {
            return Err(RemoveError{index, len})
        }
        unsafe {
            let ptr = self.as_mut_ptr().add(index);
            let ret = ptr::read(ptr);
            ptr::copy(ptr.add(1), ptr, len - index - 1);
            self.set_len(len - 1);
            Ok(ret)
        }
    }

    #[inline] unsafe fn remove_unchecked(&mut self, index: usize) -> T {
        let len = self.len();
        let ptr = self.as_mut_ptr().add(index);
        let ret = ptr::read(ptr);
        ptr::copy(ptr.add(1), ptr, len - index - 1);
        self.set_len(len - 1);
        ret
    }

    #[inline] fn try_swap_remove(&mut self, index: usize) -> Result<T, RemoveError> {
        let len = self.len();
        if index >= len {
            return Err(RemoveError{index, len});
        }
        unsafe {
            let value = ptr::read(self.as_ptr().add(index));
            let base_ptr = self.as_mut_ptr();
            ptr::copy(base_ptr.add(len - 1), base_ptr.add(index), 1);
            self.set_len(len - 1);
            Ok(value)
        }
    }

    #[inline] fn try_insert(&mut self, index: usize, element: T) -> Result<&mut T, InsertError> {
        let len = self.len();
        if index > len {
            return Err(InsertError::Index{index, len});
        }
        if len == self.capacity() {
            self.try_reserve(1).map_err(InsertError::Alloc)?;
        }
        unsafe {
            let p = self.as_mut_ptr().add(index);
            ptr::copy(p, p.add(1), len - index);
            ptr::write(p, element);
            self.set_len(len + 1);
            Ok(&mut *p)
        }
    }

    fn push_unique(&mut self, value: T, f: impl Fn(&T, &T) -> bool) -> bool {
        if self.iter().any(|x| f(x, &value)) {return false}
        self.push(value);
        true
    }

    fn push_sorted(&mut self, value: T) -> usize where T: Ord {
        let id = self.binary_search(&value).unwrap_or_else(|x| x);
        self.insert(id, value);
        id
    }

    fn push_sorted_by(&mut self, value: T, f: impl Fn(&T, &T) -> Ordering) -> usize {
        let id = self.binary_search_by(|x| f(&value, x)).unwrap_or_else(|x| x);
        self.insert(id, value);
        id
    }

    fn push_sorted_by_key<K: Ord>(&mut self, value: T, mut f: impl FnMut(&T) -> K) -> usize {
        let id = self.binary_search_by_key(&f(&value), f).unwrap_or_else(|x| x);
        self.insert(id, value);
        id
    }
}

pub trait Take: Default {
    /// replaces the value with a default one and returns the previous value
    #[inline] fn take(&mut self) -> Self {take(self)}
}
impl<T: Default> Take for T {}

pub trait RangeExt<T> {
    fn ordered(self) -> Self where T: Ord;
    fn overlap<O>(&self, other: &Range<O>) -> bool
    where O: PartialOrd<T>, T: PartialOrd<O>;
    fn loose_contain<O, I>(&self, item: I, offset: O) -> bool where
    O: Copy,
    I: PartialOrd<T> + Add<O, Output=I> + Sub<O, Output=I> + Copy,
    T: PartialOrd<I>;
    fn fit<R>(&self, item: R) -> R where T: Clone + Into<R> + PartialOrd<R>;
    /// if `value` is outside of `self`, extend `self` just enough for `value` to be inside it
    fn extend<R>(self, value: R) -> Self where T: PartialOrd<R> + From<R>;
    /// turns `x .. y` into `f(x) .. f(y)`
    fn map<R>(self, f: impl FnMut(T) -> R) -> Range<R>;
    fn to_pair(self) -> [T; 2];
}

impl<T> RangeExt<T> for Range<T> {
    #[inline] fn ordered(self) -> Self where T: Ord {
        if self.start > self.end {self.end .. self.start} else {self}
    }

    #[inline] fn overlap<O>(&self, other: &Range<O>) -> bool
    where O: PartialOrd<T>, T: PartialOrd<O> {
        self.contains(&other.start)
            || self.contains(&other.end)
            || other.contains(&self.start)
    }

    #[inline] fn loose_contain<O, I>(&self, item: I, offset: O) -> bool
    where O: Copy,
    I: PartialOrd<T> + Add<O, Output=I> + Sub<O, Output=I> + Copy,
    T: PartialOrd<I> {
        self.overlap(&(item - offset .. item + offset))
    }

    #[inline] fn fit<R>(&self, item: R) -> R
    where T: Clone + Into<R> + PartialOrd<R> {
        if      self.end  <= item {self.end.clone().into()}
        else if self.start > item {self.start.clone().into()}
        else                       {item}
    }

    #[inline] fn extend<R>(self, value: R) -> Self
    where T: PartialOrd<R> + From<R> {
        if      self.start > value {value.into() .. self.end}
        else if self.end  <= value {self.start .. value.into()}
        else {self}
    }

    #[inline] fn map<R>(self, mut f: impl FnMut(T) -> R) -> Range<R> {
        f(self.start) .. f(self.end)
    }

    #[inline] fn to_pair(self) -> [T; 2] {[self.start, self.end]}
}

#[test]
fn range_overlap() {
    assert!(!(50 .. 55).overlap(&(56 .. 61)));
    assert!(!(50 .. 56).overlap(&(56 .. 61)));
    assert!( (50 .. 57).overlap(&(56 .. 61)));
    assert!( (58 .. 60).overlap(&(56 .. 61)));
    assert!( (56 .. 61).overlap(&(58 .. 60)));
    assert!(!(56 .. 61).overlap(&(61 .. 67)));
}

pub trait LooseEq<O = Self> {
    fn loose_eq(&self, value: Self, off: O) -> bool;
    #[inline] fn loose_ne(&self, value: Self, off: O) -> bool
    where Self: Sized {
        !self.loose_eq(value, off)
    }
}

impl<O: Copy, T: PartialOrd + Add<O, Output=Self> + Sub<O, Output=Self> + Copy> LooseEq<O> for T {
    #[inline] fn loose_eq(&self, value: Self, off: O) -> bool {
        (value - off .. value + off).contains(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {pub x: i32, pub y: i32}

impl From<[R64; 2]> for Point {
    #[inline] fn from(value: [R64; 2]) -> Self {
        Self{x: value[0].into(), y: value[1].into()}
    }
}

impl<D> ArrayFrom<Point, 2> for D where D: From<i32> {
    #[inline] fn array_from(x: Point) -> [Self; 2] {[x.x.into(), x.y.into()]}
}

impl Add for Point {
    type Output = Self;
    #[inline] fn add(self, rhs: Self) -> Self::Output {
        self.checked_add(rhs).report_none().unwrap_or(self)
    }
}

impl AddAssign for Point {
    #[inline] fn add_assign(&mut self, rhs: Self) {
        self.checked_add_assign(rhs).report_false();
    }
}

impl Sub for Point {
    type Output = Self;
    #[inline] fn sub(self, rhs: Self) -> Self::Output {
        self.checked_sub(rhs).report_none().unwrap_or(self)
    }
}

impl SubAssign for Point {
    #[inline] fn sub_assign(&mut self, rhs: Self) {
        self.checked_sub_assign(rhs).report_false();
    }
}

impl Neg for Point {
    type Output = Self;
    #[inline] fn neg(self) -> Self::Output {
        self.checked_neg().report_none().unwrap_or(self)
    }
}

impl LooseEq for Point {
    #[inline] fn loose_eq(&self, value: Self, off: Self) -> bool {
        self.x.loose_eq(value.x, off.x)
            && self.y.loose_eq(value.y, off.y)
    }
}

impl RoundTo for Point {
    #[inline] fn floor_to(self, step: Self) -> Self {
        Self{x: self.x.floor_to(step.x), y: self.y.floor_to(step.y)}
    }

    #[inline] fn ceil_to(self, step: Self) -> Self {
        Self{x: self.x.ceil_to(step.x), y: self.y.ceil_to(step.y)}
    }
}

impl Point {
    pub const ZERO: Self = Self{x:0, y:0};

    #[inline] pub fn is_zero(&self) -> bool {self.x == 0 && self.y == 0}
    #[inline] pub fn nonzero(self) -> Option<Self> {if self.is_zero() {None} else {Some(self)}}

    #[inline] pub fn normalise(mut self, old_space: Rect, new_space: Rect) -> Self {
        self.y = (((self.y - old_space.bottom()) as f32 / old_space.height() as f32)
            * new_space.height() as f32) as i32 + new_space.bottom();
        self.x = (((self.x - old_space.left()) as f32 / old_space.width() as f32)
            * new_space.width() as f32) as i32 + new_space.left();
        self
    }

    #[inline] pub fn map<T>(self, mut f: impl FnMut(i32) -> T) -> [T; 2] {
        [f(self.x), f(self.y)]
    }

    #[inline] pub fn checked_add(self, rhs: Self) -> Option<Self> {
        Some(Self{x: self.x.checked_add(rhs.x)?, y: self.y.checked_add(rhs.y)?})
    }

    #[inline] pub fn checked_add_assign(&mut self, rhs: Self) -> bool {
        if let Some(x) = self.checked_add(rhs) {*self = x; true}
        else {false}
    }

    #[inline] pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        Some(Self{x: self.x.checked_sub(rhs.x)?, y: self.y.checked_sub(rhs.y)?})
    }

    #[inline] pub fn checked_sub_assign(&mut self, rhs: Self) -> bool {
        if let Some(x) = self.checked_sub(rhs) {*self = x; true}
        else {false}
    }

    #[inline] pub fn checked_neg(self) -> Option<Self> {
        Some(Self{x: self.x.checked_neg()?, y: self.y.checked_neg()?})
    }

    #[inline] pub fn checked_neg_assign(&mut self) -> bool {
        if let Some(x) = self.checked_neg() {*self = x; true}
        else {false}
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Rect(Point, Point);

impl Rect {
    #[inline] fn    left(&self) -> i32 {self.0.x}
    #[inline] fn  bottom(&self) -> i32 {self.1.y}
    #[inline] fn   width(&self) -> i32 {self.1.x - self.0.x}
    #[inline] fn  height(&self) -> i32 {self.1.y - self.0.y}
}

pub trait RoundTo {
    fn floor_to(self, step: Self) -> Self;
    fn  ceil_to(self, step: Self) -> Self;
}

macro_rules! round_to_4ints {
    ($($int:ty)+) => {$(
        impl RoundTo for $int {
            #[inline] fn floor_to(self, step: Self) -> Self {self - self % step}
            #[inline] fn  ceil_to(self, step: Self) -> Self {step - (self - 1) % step + self - 1}
        }
    )+};
}

round_to_4ints!(i8 u8 i16 u16 i32 u32 isize usize i64 u64);

#[derive(Debug)]
pub struct NanError;

impl Display for NanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "attempted to convert a NaN to a real")
    }
}

macro_rules! real_from_unsigned_ints_impl {
    ($real:ty { $float:ty } : $($nonzero:ty{ $int:ty }),+) => {
        $(
            impl From<$int> for $real {
                #[inline] fn from(x: $int) -> Self {Self(x as $float)}
            }

            impl IntoPropValue<$real> for $int {
                #[inline] fn into_prop_value(self) -> $real {self.into()}
            }

            impl From<$real> for $int {
                #[inline] fn from(x: $real) -> Self {
                    if      *x >= <$int>::MAX as $float {<$int>::MAX}
                    else if *x <= <$int>::MIN as $float {<$int>::MIN}
                    else {*x as $int}
                }
            }

            impl IntoPropValue<$int> for $real {
                #[inline] fn into_prop_value(self) -> $int {self.into()}
            }

            impl PartialEq<$int> for $real {
                #[inline] fn eq(&self, other: &$int) -> bool {
                    PartialEq::eq(&self.0, &(*other as $float))
                }
            }

            impl PartialOrd<$int> for $real {
                #[inline] fn partial_cmp(&self, other: &$int) -> Option<Ordering> {
                    PartialOrd::partial_cmp(&self.0, &(*other as $float))
                }
            }

            impl From<$nonzero> for $real {
                #[inline] fn from(x: $nonzero) -> Self {Self(x.get() as $float)}
            }

            impl IntoPropValue<$real> for $nonzero {
                #[inline] fn into_prop_value(self) -> $real {self.into()}
            }

            impl From<$real> for $nonzero {
                #[inline] fn from(x: $real) -> Self {
                    if      *x >= <$int>::MAX as $float {<$nonzero>::MAX}
                    else if *x <= 1.0 {<$nonzero>::MIN}
                    else {unsafe{<$nonzero>::new_unchecked(*x as $int)}}
                }
            }

            impl IntoPropValue<$nonzero> for $real {
                #[inline] fn into_prop_value(self) -> $nonzero {self.into()}
            }

            impl PartialEq<$nonzero> for $real {
                #[inline] fn eq(&self, other: &$nonzero) -> bool {
                    PartialEq::eq(&self.0, &(other.get() as $float))
                }
            }

            impl PartialOrd<$nonzero> for $real {
                #[inline] fn partial_cmp(&self, other: &$nonzero) -> Option<Ordering> {
                    PartialOrd::partial_cmp(&self.0, &(other.get() as $float))
                }
            }
        )+
    };
}

macro_rules! real_from_signed_ints_impl {
    ($real:ty { $float:ty } : $($nonzero:ty{ $int:ty }),+) => {
        $(
            impl From<$int> for $real {
                #[inline] fn from(x: $int) -> Self {Self(x as $float)}
            }

            impl IntoPropValue<$real> for $int {
                #[inline] fn into_prop_value(self) -> $real {self.into()}
            }

            impl From<$real> for $int {
                #[inline] fn from(x: $real) -> Self {
                    if      *x >= <$int>::MAX as $float {<$int>::MAX}
                    else if *x <= <$int>::MIN as $float {<$int>::MIN}
                    else {*x as $int}
                }
            }

            impl IntoPropValue<$int> for $real {
                #[inline] fn into_prop_value(self) -> $int {self.into()}
            }

            impl PartialEq<$int> for $real {
                #[inline] fn eq(&self, other: &$int) -> bool {
                    PartialEq::eq(&self.0, &(*other as $float))
                }
            }

            impl PartialOrd<$int> for $real {
                #[inline] fn partial_cmp(&self, other: &$int) -> Option<Ordering> {
                    PartialOrd::partial_cmp(&self.0, &(*other as $float))
                }
            }

            impl From<$nonzero> for $real {
                #[inline] fn from(x: $nonzero) -> Self {Self(x.get() as $float)}
            }

            impl IntoPropValue<$real> for $nonzero {
                #[inline] fn into_prop_value(self) -> $real {self.into()}
            }

            impl TryFrom<$real> for $nonzero {
                type Error = TryFromIntError;
                #[inline] fn try_from(x: $real) -> Result<Self, Self::Error> {
                    <$nonzero>::try_from(<$int>::from(x))
                }
            }

            impl PartialEq<$nonzero> for $real {
                #[inline] fn eq(&self, other: &$nonzero) -> bool {
                    PartialEq::eq(&self.0, &(other.get() as $float))
                }
            }

            impl PartialOrd<$nonzero> for $real {
                #[inline] fn partial_cmp(&self, other: &$nonzero) -> Option<Ordering> {
                    PartialOrd::partial_cmp(&self.0, &(other.get() as $float))
                }
            }
        )+
    };
}

macro_rules! impl_op {
    ($op:ident :: $method:ident ($self:ident : $real:ty , $rhs:ident : $rhs_ty:ty) -> $out:ty { $($s:stmt);+ }) => {
        impl $op<$rhs_ty> for $real {
            type Output = $out;
            #[inline] fn $method($self, $rhs: $rhs_ty) -> Self::Output { $($s)+ }
        }

        impl $op<&$rhs_ty> for $real {
            type Output = $out;
            #[inline] fn $method(self, rhs: &$rhs_ty) -> Self::Output {$op::$method(self, *rhs)}
        }

        impl<'a> $op<$rhs_ty> for &'a $real {
            type Output = $out;
            #[inline] fn $method(self, rhs: $rhs_ty) -> Self::Output {$op::$method(*self, rhs)}
        }

        impl<'a> $op<&$rhs_ty> for &'a $real {
            type Output = $out;
            #[inline] fn $method(self, rhs: &$rhs_ty) -> Self::Output {$op::$method(*self, *rhs)}
        }
    };
}

macro_rules! impl_assign_op {
    ($op:ident :: $method:ident ($self:ident : $real:ty , $rhs:ident : $rhs_ty:ty) { $($s:stmt);+ }) => {
        impl $op<$rhs_ty> for $real {
            #[inline] fn $method(&mut $self, $rhs: $rhs_ty) { $($s)+ }
        }

        impl $op<&$rhs_ty> for $real {
            #[inline] fn $method(&mut self, rhs: &$rhs_ty) {$op::$method(self, *rhs)}
        }
    };
}

macro_rules! real_float_operator_impl {
    ($real:ty { $float:ty } , $other_float:ty : $($op:ident :: $method:ident | $assign_op:ident :: $assign_method:ident),+) => {
        $(
            impl_op!($op::$method(self: $real, rhs: $other_float) -> $real {
                let res = self.0.$method(rhs as $float);
                if res.is_nan() {report_err(js_sys::Error::new("result is NaN").into()); self}
                else {Self(res)}
            });

            impl_assign_op!($assign_op::$assign_method(self: $real, rhs: $other_float) {
                let res = self.0.$method(rhs as $float);
                if res.is_nan() {report_err(js_sys::Error::new("result is NaN").into())}
                else {self.0 = res}
            });
        )+
    }
}

macro_rules! real_int_operator_impl {
    ($real:ty { $float:ty }, $nonzero:ty { $int:ty } : $($op:ident :: $method:ident | $assign_op:ident :: $assign_method:ident),+) => {
        $(
            impl_op!($op::$method(self: $real, rhs: $int) -> $real {
                Self(self.0.$method(rhs as $float))
            });

            impl_assign_op!($assign_op::$assign_method(self: $real, rhs: $int) {
                self.0.$assign_method(rhs as $float)
            });

            impl_op!($op::$method(self: $real, rhs: $nonzero) -> $real {
                Self(self.0.$method(rhs.get() as $float))
            });

            impl_assign_op!($assign_op::$assign_method(self: $real, rhs: $nonzero) {
                self.0.$assign_method(rhs.get() as $float)
            });
        )+
    };
}

macro_rules! real_real_operator_impl {
    ($real:ty { $float:ty } , $other_real:ty { $other_float:ty } : $($op:ident :: $method:ident | $assign_op:ident :: $assign_method:ident),+) => {
        $(
            impl_op!($op::$method(self: $real, rhs: $other_real) -> $real {
                let res = self.0.$method(rhs.0 as $float);
                if res.is_nan() {report_err(js_sys::Error::new("result is NaN").into()); self}
                else {Self(res)}
            });

            impl_assign_op!($assign_op::$assign_method(self: $real, rhs: $other_real) {
                let res = self.0.$method(rhs.0 as $float);
                if res.is_nan() {report_err(js_sys::Error::new("result is NaN").into())}
                else {self.0 = res}
            });
        )+
    }
}

macro_rules! real_impl {
    ($real:ident { $float:ident }, $other_real:ty { $other_float:ty }) => {
        #[derive(Debug, Default, PartialEq, Clone, Copy)]
        pub struct $real($float);

        impl Deref for $real {
            type Target = $float;
            #[inline] fn deref(&self) -> &Self::Target {&self.0}
        }

        impl PartialEq<$float> for $real {
            #[inline] fn eq(&self, other: &$float) -> bool {
                self.0.eq(other)
            }
        }

        impl PartialOrd<$float> for $real {
            #[inline] fn partial_cmp(&self, other: &$float) -> Option<Ordering> {
                self.0.partial_cmp(&other)
            }
        }

        impl PartialOrd for $real {
            #[inline] fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $real {
            #[inline] fn cmp(&self, other: &Self) -> Ordering {
                unsafe{self.0.partial_cmp(&other.0).unwrap_unchecked()}
            }
        }

        impl Eq for $real {}

        impl TryFrom<$float> for $real {
            type Error = NanError;
            #[inline] fn try_from(x: $float) -> Result<Self, Self::Error> {
                Self::new(x).ok_or(NanError)
            }
        }

        impl From<$other_real> for $real {
            #[inline] fn from(x: $other_real) -> Self {Self(x.0 as $float)}
        }

        impl IntoPropValue<$other_real> for $real {
            #[inline] fn into_prop_value(self) -> $other_real {self.into()}
        }

        impl Display for $real {
            #[inline] fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                Display::fmt(&self.0, f)
            }
        }

        impl Neg for $real {
            type Output = Self;
            #[inline] fn neg(self) -> Self::Output {Self(-self.0)}
        }

        impl RoundTo for $real {
            #[inline] fn floor_to(self, step: Self) -> Self {
                if self.is_infinite() || step == 0 {return self}
                Self(*self - *self % *step)
            }

            #[inline] fn  ceil_to(self, step: Self) -> Self {
                if self.is_infinite() || step == 0 {return self}
                let prev = *self - 1.0;
                Self(*step - prev % *step + prev)
            }
        }

        impl Sum for $real {
            #[inline] fn sum<I>(iter: I) -> Self where I: Iterator<Item=$real> {
                iter.fold(Self(0.0), |s, n| s + n)
            }
        }

        impl<'a> Sum<&'a $real> for $real {
            #[inline] fn sum<I>(iter: I) -> Self where I: Iterator<Item=&'a $real> {
                iter.fold(Self(0.0), |s, n| s + n)
            }
        }

        real_from_unsigned_ints_impl!($real{$float}:
            NonZeroU8{u8}, NonZeroU16{u16}, NonZeroU32{u32}, NonZeroUsize{usize}, NonZeroU64{u64});
        real_from_signed_ints_impl!($real{$float}:
            NonZeroI8{i8}, NonZeroI16{i16}, NonZeroI32{i32}, NonZeroIsize{isize}, NonZeroI64{i64});

        real_int_operator_impl!($real{$float}, NonZeroU8{u8}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_int_operator_impl!($real{$float}, NonZeroI8{i8}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_int_operator_impl!($real{$float}, NonZeroU16{u16}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_int_operator_impl!($real{$float}, NonZeroI16{i16}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_int_operator_impl!($real{$float}, NonZeroU32{u32}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_int_operator_impl!($real{$float}, NonZeroI32{i32}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_int_operator_impl!($real{$float}, NonZeroUsize{usize}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_int_operator_impl!($real{$float}, NonZeroIsize{isize}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_int_operator_impl!($real{$float}, NonZeroU64{u64}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_int_operator_impl!($real{$float}, NonZeroI64{i64}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);

        real_float_operator_impl!($real{$float}, $float:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_float_operator_impl!($real{$float}, $other_float:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_real_operator_impl!($real{$float}, $real{$float}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);
        real_real_operator_impl!($real{$float}, $other_real{$other_float}:
            Add::add|AddAssign::add_assign, Sub::sub|SubAssign::sub_assign,
            Mul::mul|MulAssign::mul_assign, Div::div|DivAssign::div_assign,
            Rem::rem|RemAssign::rem_assign);

        impl $real {
            pub const INFINITY: $real = $real($float::INFINITY);
            pub const NEG_INFINITY: $real = $real($float::NEG_INFINITY);
            pub const ZERO: $real = $real(0.0);
            pub const ONE: $real = $real(1.0);
            pub const PI: $real = $real(std::$float::consts::PI);
            pub const TAU: $real = $real(std::$float::consts::TAU);

            #[inline] pub const fn new(x: $float) -> Option<Self> {
                if x.is_nan() {None} else {Some(Self(x))}
            }

            #[inline] pub const unsafe fn new_unchecked(x: $float) -> Self {Self(x)}

            #[inline] pub const fn new_or(default: Self, x: $float) -> Self {
                if x.is_nan() {default} else {Self(x)}
            }

            #[inline] pub fn rem_euclid(self, rhs: Self) -> Option<Self> {
                let res = self.0.rem_euclid(rhs.0);
                if res.is_nan() {return None}
                Some(Self(res))
            }

            #[inline] pub fn copysign(self, sign: Self) -> Self {
                Self(self.0.copysign(*sign))
            }

            #[inline] pub fn recip(self) -> Self {Self(self.0.recip())}

            #[inline] pub fn exp2(self) -> Self {Self(self.0.exp2())}

            #[inline] pub fn floor(self) -> Self {Self(self.0.floor())}

            #[inline] pub fn ceil(self) -> Self {Self(self.0.ceil())}

            #[inline] pub fn round(self) -> Self {Self(self.0.round())}

            #[inline] pub fn is_finite(&self) -> bool {self.0.is_finite()}

            #[inline] pub fn abs(self) -> Self {Self(self.0.abs())}

            #[inline] pub fn sin(self) -> Option<Self> {Self::new(self.0.sin())}

            #[inline] pub unsafe fn sin_unchecked(self) -> Self {Self(self.0.sin())}

            #[inline] pub fn sin_or(self, default: Self) -> Self {
                Self::new(self.0.sin()).unwrap_or(default)
            }

            #[inline] pub fn cos(self) -> Option<Self> {Self::new(self.0.cos())}

            #[inline] pub unsafe fn cos_unchecked(self) -> Self {Self(self.0.cos())}

            #[inline] pub fn cos_or(self, default: Self) -> Self {
                Self::new(self.0.cos()).unwrap_or(default)
            }
        }
    };
}

real_impl!(R32{f32}, R64{f64});
real_impl!(R64{f64}, R32{f32});

#[macro_export]
macro_rules! r32 {
    ($x:literal) => {{
        #[allow(unused_unsafe)]
        unsafe{$crate::R32::new_unchecked($x)}
    }};
}

#[macro_export]
macro_rules! r64 {
    ($x:literal) => {{
        #[allow(unused_unsafe)]
        unsafe{$crate::R64::new_unchecked($x)}
    }};
}