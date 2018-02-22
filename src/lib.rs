#[macro_use]
extern crate bitflags;
extern crate libc;

pub mod ffi;

use std::ops::Drop;
use std::path::Path as IoPath;
use std::ffi::{NulError, CString};
use libc::{c_int, c_float, c_uchar, c_char};

#[cfg(target_os = "windows")]
fn init_gl() -> Result<(), ()> {
    if unsafe { ffi::gladLoadGL() } == 1 {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(not(target_os = "windows"))]
fn init_gl() -> Result<(), ()> {
    Ok(())
}

#[cfg(feature = "gl3")]
fn create_gl(flags: ffi::NVGcreateFlags) -> *mut ffi::NVGcontext {
    unsafe { ffi::nvgCreateGL3(flags.bits()) }
}

#[cfg(feature = "gl2")]
fn create_gl(flags: ffi::NVGcreateFlags) -> *mut ffi::NVGcontext {
    unsafe { ffi::nvgCreateGL2(flags.bits()) }
}

#[cfg(feature = "gles3")]
fn create_gl(flags: ffi::NVGcreateFlags) -> *mut ffi::NVGcontext {
    unsafe { ffi::nvgCreateGLES3(flags.bits()) }
}

#[cfg(feature = "gles2")]
fn create_gl(flags: ffi::NVGcreateFlags) -> *mut ffi::NVGcontext {
    unsafe { ffi::nvgCreateGLES2(flags.bits()) }
}

#[cfg(not(any(feature = "gl3", feature = "gl2", feature = "gles3", feature = "gles2")))]
fn create_gl(flags: ffi::NVGcreateFlags) -> *mut ffi::NVGcontext {
    panic!("Unable to determine the backend / implementation. Have you enabled one of the features?")
}

/// A builder that configures and constructs a NanoVG context.
#[derive(Debug)]
pub struct ContextBuilder {
    flags: ffi::NVGcreateFlags,
}

impl ContextBuilder {
    /// Create a new context builder.
    pub fn new() -> Self {
        Self {
            flags: ffi::NVGcreateFlags::empty(),
        }
    }

    /// Enable antialiased rasterization. Not needed if you have multisampling enabled.
    pub fn antialias(mut self) -> Self {
        self.flags.insert(ffi::NVGcreateFlags::NVG_ANTIALIAS);
        self
    }

    /// Enable stencil strokes. Overlapping, stroked paths will only be drawn once, for a little performance loss.
    pub fn stencil_strokes(mut self) -> Self {
        self.flags.insert(ffi::NVGcreateFlags::NVG_STENCIL_STROKES);
        self
    }

    /// Enable additional debug checks.
    pub fn debug(mut self) -> Self {
        self.flags.insert(ffi::NVGcreateFlags::NVG_DEBUG);
        self
    }

    /// Construct the context.
    /// Make sure you have enabled one of the 4 OpenGL features, or this function will panic.
    pub fn build(self) -> Result<Context, ()> {
        init_gl()?;
        let raw = create_gl(self.flags);
        if !raw.is_null() {
            Ok(Context(raw))
        } else {
            Err(())
        }
    }
}

/// A initialized NanoVG context - the central type which all operations rely on.
#[derive(Debug)]
pub struct Context(*mut ffi::NVGcontext);

impl Context {
    /// Return the raw FFI C-struct pointer to the context.
    pub fn raw(&self) -> *mut ffi::NVGcontext {
        self.0
    }

    /// Begin drawing a frame.
    /// All NanoVG drawing takes place within a frame (except text drawing).
    ///
    /// `width` and `height` should be the width and height of the framebuffer / window client size.
    /// `device_pixel_ratio` defines the pixel ratio. NanoVG doesn't guess this automatically to allow for Hi-DPI devices.
    /// Basically, this is your hidpi factor.
    /// `handler` is the callback in which you draw your paths. You cannot draw paths outside of this callback.
    pub fn frame<F: FnOnce(Frame)>(
        &self,
        (width, height): (i32, i32),
        device_pixel_ratio: f32,
        handler: F,
    ) {
        unsafe {
            ffi::nvgBeginFrame(
                self.raw(),
                width as c_int,
                height as c_int,
                device_pixel_ratio as c_float,
            );
        }
        {
            let frame = Frame::new(self);
            handler(frame);
        }
        unsafe {
            ffi::nvgEndFrame(self.raw());
        }
    }

    fn global_composite_operation(&self, operation: CompositeOperation) {
        let ctx = self.raw();
        match operation {
            CompositeOperation::Basic(basic) => unsafe {
                ffi::nvgGlobalCompositeOperation(ctx, basic.into_raw() as c_int);
            },
            CompositeOperation::BlendFunc {
                source: src,
                destination: dst,
            } => unsafe {
                ffi::nvgGlobalCompositeBlendFunc(ctx, src.into_raw().bits(), dst.into_raw().bits());
            },
            CompositeOperation::BlendFuncSeparate {
                rgb_source: rs,
                rgb_destination: rd,
                alpha_source: als,
                alpha_destination: ald,
            } => unsafe {
                let (rs, rd, als, ald) = (
                    rs.into_raw().bits(),
                    rd.into_raw().bits(),
                    als.into_raw().bits(),
                    ald.into_raw().bits(),
                );
                ffi::nvgGlobalCompositeBlendFuncSeparate(ctx, rs, rd, als, ald);
            },
        }
    }

    fn global_alpha(&self, alpha: f32) {
        unsafe {
            ffi::nvgGlobalAlpha(self.raw(), alpha as c_float);
        }
    }

    fn scissor(&self, scissor: Option<Scissor>) {
        if let Some(scissor) = scissor {
            match scissor {
                Scissor::Rect {
                    x,
                    y,
                    width,
                    height,
                } => unsafe {
                    ffi::nvgScissor(self.raw(), x, y, width, height);
                },
                Scissor::Intersect {
                    x,
                    y,
                    width,
                    height,
                } => unsafe {
                    ffi::nvgIntersectScissor(self.raw(), x, y, width, height);
                },
            }
        } else {
            unsafe {
                ffi::nvgResetScissor(self.raw());
            }
        }
    }

    fn text_prepare(&self, font: Font, options: TextOptions) {
        unsafe {
            ffi::nvgFontFaceId(self.raw(), font.id());
            ffi::nvgFillColor(self.raw(), options.color.into_raw());
            ffi::nvgFontSize(self.raw(), options.size);
            ffi::nvgFontBlur(self.raw(), options.blur);
            ffi::nvgTextLetterSpacing(self.raw(), options.letter_spacing);
            ffi::nvgTextLineHeight(self.raw(), options.line_height);
            ffi::nvgTextAlign(self.raw(), options.align.into_raw().bits());
        }
        self.scissor(options.scissor);
    }

    /// Draw a single line on the screen. Newline characters are ignored.
    /// `font` the font face to use.
    /// `(x, y)` the origin / position to draw the text at. The origin is relative to the alignment of `options`.
    /// `text` the string to draw.
    /// `options` optional (`Default::default`) options that control the visual appearance of the text.
    pub fn text<S: AsRef<str>>(
        &self,
        font: Font,
        (x, y): (f32, f32),
        text: S,
        options: TextOptions,
    ) {
        let text = CString::new(text.as_ref()).unwrap();
        self.text_prepare(font, options);
        unsafe {
            ffi::nvgText(self.raw(), x, y, text.into_raw(), 0 as *const _);
        }
    }

    /// Draw multiline text on the screen.
    /// `font` the font face to use.
    /// `(x, y)` the origin / position to draw the text at. The origin is relative to the alignment of `options`.
    /// `text` the string to draw.
    /// `options` optional (`Default::default`) options that control the visual appearance of the text.
    pub fn text_box<S: AsRef<str>>(
        &self,
        font: Font,
        (x, y): (f32, f32),
        text: S,
        options: TextOptions,
    ) {
        let text = CString::new(text.as_ref()).unwrap();
        self.text_prepare(font, options);
        unsafe {
            ffi::nvgTextBox(
                self.raw(),
                x,
                y,
                options.line_max_width,
                text.into_raw(),
                0 as *const _,
            );
        }
    }

    /// Measures specified text string.
    /// Returns tuple (f32, TextBounds) where the first element specifies horizontal advance of measured text
    /// and the second element specifies the bounding box of measured text.
    /// `font` the font face to use.
    /// `(x, y)` the origin / position to measure the text from.
    /// `text` the string to measure.
    /// `options` optional (`Default::default`) options that controls how the text is measured.
    pub fn text_bounds<S: AsRef<str>>(
        &self,
        font: Font,
        (x, y): (f32, f32),
        text: S,
        options: TextOptions,
    ) -> (f32, TextBounds) {
        let text = CString::new(text.as_ref()).unwrap();
        self.text_prepare(font, options);
        let mut bounds = [0.0f32; 4];
        let measure = unsafe {
            ffi::nvgTextBounds(
                self.raw(),
                x,
                y,
                text.into_raw(),
                0 as *const _,
                bounds.as_mut_ptr(),
            )
        };
        (measure, TextBounds::new(&bounds))
    }

    /// Measures specified multi-text string.
    /// Returns bounding box of measured multi-text.
    /// `font` the font face to use.
    /// `(x, y)` the origin / position to measure the text from.
    /// `text` the string to measure.
    /// `options` optional (`Default::default`) options that controls how the text is measured.
    pub fn text_box_bounds<S: AsRef<str>>(
        &self,
        font: Font,
        (x, y): (f32, f32),
        text: S,
        options: TextOptions,
    ) -> TextBounds {
        let text = CString::new(text.as_ref()).unwrap();
        self.text_prepare(font, options);
        let mut bounds = [0.0f32; 4];
        unsafe {
            ffi::nvgTextBoxBounds(
                self.raw(),
                x,
                y,
                options.line_max_width,
                text.into_raw(),
                0 as *const _,
                bounds.as_mut_ptr(),
            )
        }
        TextBounds::new(&bounds)
    }

    /// Calculates and breaks text into series of glyph positions.
    /// Returns iterator over all glyph positions in text.
    /// `(x, y)` the coordinate space from which to offset coordinates in `GlyphPosition`
    /// `text` the text to break into glyph positions
    pub fn text_glyph_positions<S: AsRef<str>>(
        &self,
        (x, y): (f32, f32),
        text: S,
    ) -> TextGlyphPositions {
        TextGlyphPositions::new(
            &self,
            x,
            y,
            CString::new(text.as_ref()).unwrap()
        )
    }

    /// Returns vertical text metrics based on given font and text options
    /// Measured values are stored in TextMetrics struct in local coordinate space.
    /// `options` the options specify how metrics should be calculated.
    /// `font` the font for which to calculate metrics.
    pub fn text_metrics(&self, font: Font, options: TextOptions) -> TextMetrics {
        self.text_prepare(font, options);
        let mut metrics = TextMetrics::new();
        unsafe {
            ffi::nvgTextMetrics(
                self.raw(),
                &mut metrics.ascender,
                &mut metrics.descender,
                &mut metrics.line_height
            );
        }
        metrics
    }

    /// Breaks text into lines.
    /// Text is split at word boundaries, new-line character or when row width exceeds break_row_width.
    /// Returns iterator over text lines.
    /// `text` the text to break into lines
    /// `break_row_width` maximum width of row
    pub fn text_break_lines<S: AsRef<str>>(
        &self,
        text: S,
        break_row_width: f32,
    ) -> TextBreakLines {
        TextBreakLines::new(
            &self,
            CString::new(text.as_ref()).unwrap(),
            break_row_width
        )
    }
}

impl Drop for Context {
    #[cfg(feature = "gl3")]
    fn drop(&mut self) {
        unsafe {
            ffi::nvgDeleteGL3(self.0);
        }
    }

    #[cfg(feature = "gl2")]
    fn drop(&mut self) {
        unsafe {
            ffi::nvgDeleteGL2(self.0);
        }
    }

    #[cfg(feature = "gles3")]
    fn drop(&mut self) {
        unsafe {
            ffi::nvgDeleteGLES3(self.0);
        }
    }

    #[cfg(feature = "gles2")]
    fn drop(&mut self) {
        unsafe {
            ffi::nvgDeleteGLES2(self.0);
        }
    }

    #[cfg(not(any(feature = "gl3", feature = "gl2", feature = "gles3", feature = "gles2")))]
    fn drop(&mut self) {}
}

/// A scissor defines a region on the screen in which drawing operations are allowed.
/// Pixels drawn outside of this region are clipped.
#[derive(Clone, Copy, Debug)]
pub enum Scissor {
    /// Defines a rectangular scissor.
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    /// Define the scissor to be the intersection between the previous scissor rectangle
    /// and the specified rectangle.
    /// The previous and specified rectangles are always transformed to be in the previous transform space.
    Intersect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

/// Options which control how a path is rendered.
#[derive(Debug)]
pub struct PathOptions {
    /// The scissor defines the rectangular boundary in which the frame is clipped into.
    /// All overflowing pixels will be discarded.
    pub scissor: Option<Scissor>,
    /// Defines how overlapping paths are composited together.
    pub composite_operation: CompositeOperation,
    /// The alpha component of the path.
    pub alpha: f32,
    /// A transformation which 'transforms' the coordinate system and consequently the path.
    pub transform: Option<Transform>,
}

impl Default for PathOptions {
    fn default() -> Self {
        Self {
            scissor: None,
            composite_operation: CompositeOperation::Basic(BasicCompositeOperation::Atop),
            alpha: 1.0,
            transform: None,
        }
    }
}

/// A frame which can draw paths.
/// All NanoVG path drawing operations are done on a frame.
#[derive(Debug)]
pub struct Frame<'a> {
    context: &'a Context,
}

impl<'a> Frame<'a> {
    fn new(context: &'a Context) -> Self {
        Self { context }
    }

    /// Get the underlying context this frame was created on.
    pub fn context(&self) -> &'a Context {
        self.context
    }

    /// Draw a new path.
    ///
    /// `handler` is the callback in which you operate the path.
    /// `options` control how the path is rendered.
    pub fn path<F: FnOnce(Path)>(&self, handler: F, options: PathOptions) {
        self.context.global_composite_operation(options.composite_operation);
        self.context.global_alpha(options.alpha);
        self.context.scissor(options.scissor);

        if let Some(t) = options.transform {
            let t = t.matrix;
            unsafe { ffi::nvgTransform(self.context.raw(), t[0], t[1], t[2], t[3], t[4], t[5]); }
        }

        unsafe { ffi::nvgBeginPath(self.context.raw()); }
        handler(Path::new(self));

        if options.transform.is_some() {
            unsafe { ffi::nvgResetTransform(self.context.raw()); }
        }
    }
}

/// A path, the main type for most NanoVG drawing operations.
#[derive(Debug)]
pub struct Path<'a, 'b>
where
    'b: 'a,
{
    frame: &'a Frame<'b>,
}

impl<'a, 'b> Path<'a, 'b> {
    fn new(frame: &'a Frame<'b>) -> Self {
        Self { frame }
    }

    fn ctx(&self) -> *mut ffi::NVGcontext {
        self.frame.context.raw()
    }

    /// Get the underlying context this path was created on.
    pub fn context(&self) -> &'a Context {
        self.frame.context()
    }

    /// Draw the current path by filling in it's shape.
    pub fn fill(&self, style: FillStyle) {
        let ctx = self.ctx();
        unsafe {
            ffi::nvgShapeAntiAlias(ctx, style.antialias as c_int);
            match style.coloring_style {
                ColoringStyle::Color(color) => ffi::nvgFillColor(ctx, color.into_raw()),
                ColoringStyle::Paint(paint) => ffi::nvgFillPaint(ctx, paint.into_raw()),
            }
            ffi::nvgFill(ctx);
        }
    }

    /// Draw the current path by stroking it's perimeter.
    pub fn stroke(&self, style: StrokeStyle) {
        let ctx = self.ctx();
        unsafe {
            ffi::nvgShapeAntiAlias(ctx, style.antialias as c_int);
            match style.coloring_style {
                ColoringStyle::Color(color) => ffi::nvgStrokeColor(ctx, color.into_raw()),
                ColoringStyle::Paint(paint) => ffi::nvgStrokePaint(ctx, paint.into_raw()),
            }
            ffi::nvgStrokeWidth(ctx, style.width as c_float);
            ffi::nvgLineCap(ctx, style.line_cap.into_raw() as c_int);
            ffi::nvgLineJoin(ctx, style.line_join.into_raw() as c_int);
            ffi::nvgMiterLimit(ctx, style.miter_limit as c_float);
            ffi::nvgStroke(ctx);
        }
    }

    /// Add an arc to the path.
    pub fn arc(
        &self,
        (cx, cy): (f32, f32),
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        winding: Winding,
    ) {
        unsafe {
            ffi::nvgArc(
                self.ctx(),
                cx,
                cy,
                radius,
                start_angle,
                end_angle,
                winding.into_raw(),
            );
        }
    }

    /// Add a rectangle to the path.
    pub fn rect(&self, (x, y): (f32, f32), (w, h): (f32, f32)) {
        unsafe {
            ffi::nvgRect(
                self.ctx(),
                x as c_float,
                y as c_float,
                w as c_float,
                h as c_float,
            );
        }
    }

    /// Add a rounded rectangle to the path.
    pub fn rounded_rect(&self, (x, y): (f32, f32), (w, h): (f32, f32), radius: f32) {
        unsafe {
            ffi::nvgRoundedRect(self.ctx(), x, y, w, h, radius);
        }
    }

    /// Add a rounded rectangle with varying corners to the path.
    /// `top_radii` and `bottom_radii` are both tuples in the form (left, right).
    pub fn rounded_rect_varying(
        &self,
        (x, y): (f32, f32),
        (w, h): (f32, f32),
        top_radii: (f32, f32),
        bottom_radii: (f32, f32),
    ) {
        unsafe {
            ffi::nvgRoundedRectVarying(
                self.ctx(),
                x,
                y,
                w,
                h,
                top_radii.0,
                top_radii.1,
                bottom_radii.1,
                bottom_radii.0,
            );
        }
    }

    /// Add an ellipse to the path.
    pub fn ellipse(&self, (cx, cy): (f32, f32), radius_x: f32, radius_y: f32) {
        unsafe {
            ffi::nvgEllipse(self.ctx(), cx, cy, radius_x, radius_y);
        }
    }

    /// Add a circle to the path.
    pub fn circle(&self, (cx, cy): (f32, f32), radius: f32) {
        unsafe {
            ffi::nvgCircle(self.ctx(), cx, cy, radius);
        }
    }

    /// Add subpath to the path. A subpath is a custom shape.
    pub fn sub_path<F: FnOnce(SubPath)>(&self, (x, y): (f32, f32), handler: F) {
        let ctx = self.ctx();
        unsafe {
            ffi::nvgMoveTo(ctx, x, y);
        }
        handler(SubPath::new(self));
    }
}

/// A custom shape defined by lines, arcs and curves.
#[derive(Debug)]
pub struct SubPath<'a, 'b, 'c>
where
    'b: 'a,
    'c: 'b,
{
    path: &'a Path<'b, 'c>,
}

impl<'a, 'b, 'c> SubPath<'a, 'b, 'c> {
    fn new(path: &'a Path<'b, 'c>) -> Self {
        Self { path }
    }

    fn ctx(&self) -> *mut ffi::NVGcontext {
        self.path.ctx()
    }

    /// Get the underlying context this sub-path was created on.
    pub fn context(&self) -> &'a Context {
        self.path.context()
    }

    /// Add a line to the subpath.
    pub fn line_to(&self, (x, y): (f32, f32)) {
        unsafe {
            ffi::nvgLineTo(self.ctx(), x, y);
        }
    }

    /// Add a cubic bezier curve to the subpath.
    pub fn cubic_bezier_to(&self, (x, y): (f32, f32), control1: (f32, f32), control2: (f32, f32)) {
        unsafe {
            ffi::nvgBezierTo(
                self.ctx(),
                control1.0,
                control1.1,
                control2.0,
                control2.1,
                x,
                y,
            );
        }
    }

    /// Add a quadratic bezier curve to the subpath.
    pub fn quad_bezier_to(&self, (x, y): (f32, f32), control: (f32, f32)) {
        unsafe {
            ffi::nvgQuadTo(self.ctx(), control.0, control.1, x, y);
        }
    }

    /// Add a arc to the subpath.
    pub fn arc_to(&self, p1: (f32, f32), p2: (f32, f32), radius: f32) {
        unsafe {
            ffi::nvgArcTo(self.ctx(), p1.0, p1.1, p2.0, p2.1, radius);
        }
    }

    /// Set the winding of the subpath.
    /// The winding defines which parts of the subpath are 'inside' and which are 'outside'.
    pub fn winding(&self, winding: Winding) {
        unsafe {
            ffi::nvgPathWinding(self.ctx(), winding.into_raw());
        }
    }

    /// Close the path, ie. connect the first point and last point with a line.
    pub fn close(&self) {
        unsafe {
            ffi::nvgClosePath(self.ctx());
        }
    }
}

/// Controls how filling in a path should look.
#[derive(Debug)]
pub struct FillStyle {
    pub coloring_style: ColoringStyle,
    pub antialias: bool,
}

impl Default for FillStyle {
    fn default() -> Self {
        Self {
            coloring_style: ColoringStyle::Color(Color::from_rgb(0, 0, 0)),
            antialias: true,
        }
    }
}

/// Controls how stroking a path should look.
#[derive(Debug)]
pub struct StrokeStyle {
    pub coloring_style: ColoringStyle,
    pub width: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub miter_limit: f32,
    pub antialias: bool,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            coloring_style: ColoringStyle::Color(Color::from_rgb(0, 0, 0)),
            width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            antialias: true,
        }
    }
}

/// Controls how the end of line is drawn.
#[derive(Clone, Copy, Debug)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

impl LineCap {
    fn into_raw(self) -> ffi::NVGlineCap {
        match self {
            LineCap::Butt => ffi::NVGlineCap::NVG_BUTT,
            LineCap::Round => ffi::NVGlineCap::NVG_ROUND,
            LineCap::Square => ffi::NVGlineCap::NVG_SQUARE,
        }
    }
}

/// Controls how lines are joined together.
#[derive(Clone, Copy, Debug)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel
}

impl LineJoin {
    fn into_raw(self) -> ffi::NVGlineCap {
        match self {
            LineJoin::Miter => ffi::NVGlineCap::NVG_MITER,
            LineJoin::Round => ffi::NVGlineCap::NVG_ROUND,
            LineJoin::Bevel => ffi::NVGlineCap::NVG_BEVEL,
        }
    }
}

/// Controls how something should be colored.
/// Either through a single, flat color; or a more complex paint.
#[derive(Debug)]
pub enum ColoringStyle {
    Color(Color),
    Paint(Paint),
}

/// A 32-bit color value.
#[derive(Clone, Copy, Debug)]
pub struct Color(ffi::NVGcolor);

impl Color {
    /// Create a new color by setting all components manually.
    /// Values are in the range 0.0...1.0.
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color(ffi::NVGcolor { rgba: [r, g, b, a] })
    }

    /// Create a new color from three 8-bit color channels.
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Color(unsafe {
            ffi::nvgRGB(r as c_uchar, g as c_uchar, b as c_uchar)
        })
    }

    /// Create a new color from three 8-bit color channels and an 8-bit alpha channel.
    pub fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color(unsafe {
            ffi::nvgRGBA(r as c_uchar, g as c_uchar, b as c_uchar, a as c_uchar)
        })
    }

    /// Create a new color from three hsl channels.
    pub fn from_hsl(h: f32, s: f32, l: f32) -> Self {
        Color(unsafe {
            ffi::nvgHSL(h as c_float, s as c_float, l as c_float)
        })
    }

    /// Create a new color from three hsl channels and an 8-bit alpha channel.
    pub fn from_hsla(h: f32, s: f32, l: f32, a: u8) -> Self {
        Color(unsafe {
            ffi::nvgHSLA(h as c_float, s as c_float, l as c_float, a as c_uchar)
        })
    }

    fn into_raw(self) -> ffi::NVGcolor {
        self.0
    }

    /// Get the red component.
    pub fn red(&self) -> f32 {
        self.0.rgba[0]
    }

    /// Get the green component.
    pub fn green(&self) -> f32 {
        self.0.rgba[1]
    }

    /// Get the blue component.
    pub fn blue(&self) -> f32 {
        self.0.rgba[2]
    }

    /// Get the alpha component.
    pub fn alpha(&self) -> f32 {
        self.0.rgba[3]
    }

    /// Set the red component.
    pub fn set_red(&mut self, red: f32) {
        self.0.rgba[0] = red;
    }

    /// Get the green component.
    pub fn set_green(&mut self, green: f32) {
        self.0.rgba[1] = green;
    }

    /// Get the blue component.
    pub fn set_blue(&mut self, blue: f32) {
        self.0.rgba[2] = blue;
    }

    /// Get the alpha component.
    pub fn set_alpha(&mut self, alpha: f32) {
        self.0.rgba[3] = alpha;
    }

    /// Create a new color by linearly interpolating between two existing colors.
    pub fn lerp(a: Color, b: Color, t: f32) -> Color {
        Color(unsafe {
            ffi::nvgLerpRGBA(a.into_raw(), b.into_raw(), t as c_float)
        })
    }
}

/// A Paint is a more complex and powerful method of defining color.
/// With it you can draw images and gradients.
#[derive(Copy, Clone, Debug)]
pub struct Paint(ffi::NVGpaint);

impl Paint {
    pub fn with_linear_gradient(
        context: &Context,
        start: (f32, f32),
        end: (f32, f32),
        start_color: Color,
        end_color: Color,
    ) -> Self {
        let ((sx, sy), (ex, ey)) = (start, end);
        Paint(unsafe {
            ffi::nvgLinearGradient(
                context.raw(),
                sx,
                sy,
                ex,
                ey,
                start_color.into_raw(),
                end_color.into_raw(),
            )
        })
    }

    pub fn with_box_gradient(
        context: &Context,
        (x, y): (f32, f32),
        (w, h): (f32, f32),
        radius: f32,
        feather: f32,
        start_color: Color,
        end_color: Color,
    ) -> Self {
        Paint(unsafe {
            ffi::nvgBoxGradient(
                context.raw(),
                x,
                y,
                w,
                h,
                radius,
                feather,
                start_color.into_raw(),
                end_color.into_raw(),
            )
        })
    }

    pub fn with_radial_gradient(
        context: &Context,
        center: (f32, f32),
        inner_radius: f32,
        outer_radius: f32,
        start_color: Color,
        end_color: Color,
    ) -> Self {
        let (cx, cy) = center;
        Paint(unsafe {
            ffi::nvgRadialGradient(
                context.raw(),
                cx,
                cy,
                inner_radius,
                outer_radius,
                start_color.into_raw(),
                end_color.into_raw(),
            )
        })
    }

    pub fn with_image_pattern(
        context: &Context,
        image: &Image,
        origin: (f32, f32),
        size: (f32, f32),
        angle: f32,
        alpha: f32,
    ) -> Self {
        let ((ox, oy), (ex, ey)) = (origin, size);
        Paint(unsafe {
            ffi::nvgImagePattern(context.raw(), ox, oy, ex, ey, angle, image.raw(), alpha)
        })
    }

    fn into_raw(self) -> ffi::NVGpaint {
        self.0
    }
}

#[derive(Debug)]
pub struct ImageBuilder<'a> {
    context: &'a Context,
    flags: ffi::NVGimageFlags,
}

impl<'a> ImageBuilder<'a> {
    fn new(context: &'a Context) -> Self {
        Self {
            context,
            flags: ffi::NVGimageFlags::empty(),
        }
    }

    /// Get the underlying context this ImageBuilder was created on.
    pub fn context(&self) -> &'a Context {
        self.context
    }

    /// Create mipmaps during the creation of the image.
    pub fn mipmaps(mut self) -> Self {
        self.flags |= ffi::NVGimageFlags::NVG_IMAGE_GENERATE_MIPMAPS;
        self
    }

    /// Repeat the image on the X axis.
    pub fn repeat_x(mut self) -> Self {
        self.flags |= ffi::NVGimageFlags::NVG_IMAGE_REPEATX;
        self
    }

    /// Repeat the image on the Y axis.
    pub fn repeat_y(mut self) -> Self {
        self.flags |= ffi::NVGimageFlags::NVG_IMAGE_REPEATY;
        self
    }

    /// Flip (invert) the image in the Y direction during rendering.
    pub fn flipy(mut self) -> Self {
        self.flags |= ffi::NVGimageFlags::NVG_IMAGE_FLIPY;
        self
    }

    /// The image data contains premultiplied alpha.
    pub fn premultiplied(mut self) -> Self {
        self.flags |= ffi::NVGimageFlags::NVG_IMAGE_PREMULTIPLIED;
        self
    }

    /// Use nearest interpolation instead of linear.
    pub fn nearest(mut self) -> Self {
        self.flags |= ffi::NVGimageFlags::NVG_IMAGE_NEAREST;
        self
    }

    /// Construct the image by loading it from an image file on the file system.
    pub fn build_from_file<P: AsRef<IoPath>>(self, file: P) -> ImageBuilderResult<'a> {
        let path = match file.as_ref().to_str() {
            Some(p) => CString::new(p.to_owned())?,
            None => return Err(ImageBuilderError::CStringError),
        };

        let handle =
            unsafe { ffi::nvgCreateImage(self.context.raw(), (*path).as_ptr(), self.flags.bits()) };
        if handle > 0 {
            Ok(Image(self.context, handle))
        } else {
            Err(ImageBuilderError::CreateImageFailed)
        }
    }

    /// Construct the image by loading it from an image file in memory.
    pub fn build_from_memory(self, data: &[u8]) -> ImageBuilderResult<'a> {
        let handle = unsafe {
            ffi::nvgCreateImageMem(
                self.context.raw(),
                self.flags.bits(),
                data.as_ptr() as *mut _,
                data.len() as c_int,
            )
        };
        if handle > 0 {
            Ok(Image(self.context, handle))
        } else {
            Err(ImageBuilderError::CreateImageFailed)
        }
    }

    /// Construct the image by filling it with pixel data from memory (always 32bit RGBA).
    pub fn build_from_rgba(
        self,
        width: usize,
        height: usize,
        data: &[u32],
    ) -> ImageBuilderResult<'a> {
        if data.len() < width * height {
            return Err(ImageBuilderError::NotEnoughData);
        }

        let handle = unsafe {
            ffi::nvgCreateImageRGBA(
                self.context.raw(),
                width as c_int,
                height as c_int,
                self.flags.bits(),
                data.as_ptr() as *const _,
            )
        };
        if handle > 0 {
            Ok(Image(self.context, handle))
        } else {
            Err(ImageBuilderError::CreateImageFailed)
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ImageBuilderError {
    /// The path for `build_from_file` could not be converted to a c-string.
    CStringError,
    /// The call to `nvgCreateImage`, or similar functions, failed.
    CreateImageFailed,
    /// For `from_rgba`, the passed data slice does not contain enough data for the specified image size.
    NotEnoughData,
}

impl From<NulError> for ImageBuilderError {
    fn from(_: NulError) -> Self {
        ImageBuilderError::CStringError
    }
}

pub type ImageBuilderResult<'a> = Result<Image<'a>, ImageBuilderError>;

/// Handle to an image.
#[derive(Debug)]
pub struct Image<'a>(&'a Context, c_int);

impl<'a> Image<'a> {
    pub fn new(context: &'a Context) -> ImageBuilder {
        ImageBuilder::new(context)
    }

    /// Get the underlying context this image was created on.
    pub fn context(&self) -> &'a Context {
        self.0
    }

    pub fn size(&self) -> (usize, usize) {
        let (mut w, mut h): (c_int, c_int) = (0, 0);
        unsafe {
            ffi::nvgImageSize(
                self.ctx().raw(),
                self.raw(),
                &mut w as *mut _,
                &mut h as *mut _,
            );
        }
        (w as usize, h as usize)
    }

    pub fn update(&mut self, data: &[u32]) {
        unsafe {
            ffi::nvgUpdateImage(self.ctx().raw(), self.raw(), data.as_ptr() as *const _);
        }
    }

    fn ctx(&self) -> &Context {
        self.0
    }

    fn raw(&self) -> c_int {
        self.1
    }
}

impl<'a> Drop for Image<'a> {
    fn drop(&mut self) {
        unsafe {
            ffi::nvgDeleteImage(self.ctx().raw(), self.raw());
        }
        self.1 = 0;
    }
}

#[derive(Copy, Clone, Debug)]
pub enum Direction {
    Clockwise,
    CounterClockwise,
}

impl Direction {
    fn into_raw(self) -> ffi::NVGwinding {
        match self {
            Direction::Clockwise => ffi::NVGwinding::NVG_CW,
            Direction::CounterClockwise => ffi::NVGwinding::NVG_CCW,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum Solidity {
    Hole,
    Solid,
}

impl Solidity {
    fn into_raw(self) -> ffi::NVGsolidity {
        match self {
            Solidity::Hole => ffi::NVGsolidity::NVG_HOLE,
            Solidity::Solid => ffi::NVGsolidity::NVG_SOLID,
        }
    }
}

/// Winding enum that holds either Direction or Solidity enum
/// These two are identical aliases.
/// They are here for different meanings in different contexts
#[derive(Debug)]
pub enum Winding {
    Direction(Direction),
    Solidity(Solidity),
}

impl Winding {
    fn into_raw(self) -> c_int {
        match self {
            Winding::Direction(direction) => direction.into_raw().bits(),
            Winding::Solidity(solidity) => solidity.into_raw().bits(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum CompositeOperation {
    Basic(BasicCompositeOperation),
    BlendFunc {
        source: BlendFactor,
        destination: BlendFactor,
    },
    BlendFuncSeparate {
        rgb_source: BlendFactor,
        rgb_destination: BlendFactor,
        alpha_source: BlendFactor,
        alpha_destination: BlendFactor,
    },
}

#[derive(Copy, Clone, Debug)]
pub enum BasicCompositeOperation {
    SourceOver,
    SourceIn,
    SourceOut,
    Atop,
    DestinationOver,
    DestinationIn,
    DestinationOut,
    DestinationAtop,
    Lighter,
    Copy,
    Xor,
}

impl BasicCompositeOperation {
    fn into_raw(self) -> ffi::NVGcompositeOperation {
        use BasicCompositeOperation::*;
        use ffi::NVGcompositeOperation::*;
        match self {
            SourceOver => NVG_SOURCE_OVER,
            SourceIn => NVG_SOURCE_IN,
            SourceOut => NVG_SOURCE_OUT,
            Atop => NVG_ATOP,
            DestinationOver => NVG_DESTINATION_OVER,
            DestinationIn => NVG_DESTINATION_IN,
            DestinationOut => NVG_DESTINATION_OUT,
            DestinationAtop => NVG_DESTINATION_ATOP,
            Lighter => NVG_LIGHTER,
            Copy => NVG_COPY,
            Xor => NVG_XOR,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum BlendFactor {
    Zero,
    One,
    SourceColor,
    OneMinusSourceColor,
    DestinationColor,
    OneMinusDestinationColor,
    SourceAlpha,
    OneMinusSourceAlpha,
    DestinationAlpha,
    OneMinusDestinationAlpha,
    SourceAlphaSaturate,
}

impl BlendFactor {
    fn into_raw(self) -> ffi::NVGblendFactor {
        use BlendFactor::*;
        match self {
            Zero => ffi::NVGblendFactor::NVG_ZERO,
            One => ffi::NVGblendFactor::NVG_ONE,
            SourceColor => ffi::NVGblendFactor::NVG_SRC_COLOR,
            OneMinusSourceColor => ffi::NVGblendFactor::NVG_ONE_MINUS_SRC_COLOR,
            DestinationColor => ffi::NVGblendFactor::NVG_DST_COLOR,
            OneMinusDestinationColor => ffi::NVGblendFactor::NVG_ONE_MINUS_DST_COLOR,
            SourceAlpha => ffi::NVGblendFactor::NVG_SRC_ALPHA,
            OneMinusSourceAlpha => ffi::NVGblendFactor::NVG_ONE_MINUS_SRC_ALPHA,
            DestinationAlpha => ffi::NVGblendFactor::NVG_DST_ALPHA,
            OneMinusDestinationAlpha => ffi::NVGblendFactor::NVG_ONE_MINUS_DST_ALPHA,
            SourceAlphaSaturate => ffi::NVGblendFactor::NVG_SRC_ALPHA_SATURATE,
        }
    }
}

/// A handle to a font.
/// Fonts are managed by the NanoVG context itself. View this type only as a 'reference' to a font.
#[derive(Clone, Copy, Debug)]
pub struct Font<'a>(&'a Context, c_int);

#[derive(Debug)]
pub enum CreateFontError {
    /// Conversion from a Rust-utf8-string to a CString failed.
    CStringError,
    /// A specified path is invalid somehow.
    InvalidPath,
    /// The font handle returned by the ffi functions is invalid.
    InvalidHandle,
}

impl From<NulError> for CreateFontError {
    fn from(_: NulError) -> Self {
        CreateFontError::CStringError
    }
}

pub type CreateFontResult<'a> = Result<Font<'a>, CreateFontError>;

impl<'a> Font<'a> {
    fn ctx(&self) -> *mut ffi::NVGcontext {
        self.0.raw()
    }

    fn id(&self) -> c_int {
        self.1
    }

    /// Get the underlying context this font was created on.
    pub fn context(&self) -> &'a Context {
        self.0
    }

    /// Attempt to load a font from the file at `path`.
    /// Fonts are always named (specified with `name`).
    pub fn from_file<S: AsRef<str>, P: AsRef<IoPath>>(
        context: &'a Context,
        name: S,
        path: P,
    ) -> CreateFontResult {
        let name = CString::new(name.as_ref())?;
        let path = CString::new(path.as_ref().to_str().ok_or(CreateFontError::InvalidPath)?)?;
        let handle = unsafe { ffi::nvgCreateFont(context.raw(), name.into_raw(), path.into_raw()) };
        if handle > ffi::FONS_INVALID {
            Ok(Font(context, handle))
        } else {
            Err(CreateFontError::InvalidHandle)
        }
    }

    /// Attempt to load a font from memory.
    /// Fonts are always named (specified with `name`).
    pub fn from_memory<'b, S: AsRef<str>>(
        context: &'a Context,
        name: S,
        memory: &'b [u8],
    ) -> CreateFontResult<'a> {
        let name = CString::new(name.as_ref())?;
        let handle = unsafe {
            ffi::nvgCreateFontMem(
                context.raw(),
                name.into_raw(),
                memory.as_ptr() as *mut _,
                memory.len() as c_int,
                0,
            )
        };
        if handle > ffi::FONS_INVALID {
            Ok(Font(context, handle))
        } else {
            Err(CreateFontError::InvalidHandle)
        }
    }

    /// Try to find a already loaded font with the given `name`.
    pub fn find<S: AsRef<str>>(context: &'a Context, name: S) -> CreateFontResult {
        let handle =
            unsafe { ffi::nvgFindFont(context.raw(), CString::new(name.as_ref())?.into_raw()) };
        if handle > ffi::FONS_INVALID {
            Ok(Font(context, handle))
        } else {
            Err(CreateFontError::InvalidHandle)
        }
    }

    /// Add `fallback` as a fallback for the current font.
    /// If the font renderer fails to rasterize a glyph with the main font, it will automatically
    /// attempt to rasterize the same glyph with the fallback font.
    /// This process continues until no working font is found, then the glyph is skipped.
    pub fn add_fallback(&self, fallback: Font) -> bool {
        let res = unsafe { ffi::nvgAddFallbackFontId(self.ctx(), self.id(), fallback.id()) };
        res != 0
    }
}

/// Options which control the visual appearance of a text.
#[derive(Clone, Copy, Debug)]
pub struct TextOptions {
    /// The size of the text in points.
    pub size: f32,
    /// The radial blur of the text, in pixels.
    pub blur: f32,
    /// How much each individual letter of the text should be apart.
    pub letter_spacing: f32,
    /// The height for each line. Specified in multiplies of the font height.
    /// Ex.: a `line_height` of 3.0 means each line is font height * 3 apart.
    pub line_height: f32,
    /// The width at which multiline text is automatically wrapped.
    pub line_max_width: f32,
    /// How to align the text.
    pub align: Alignment,
    /// The fill color of the text.
    pub color: Color,
    /// The scissor defines the rectangular boundary in which the text is clipped into.
    /// All overflowing pixels will be discarded.
    pub scissor: Option<Scissor>,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            size: 12.0,
            blur: 0.0,
            letter_spacing: 0.0,
            line_height: 1.0,
            line_max_width: std::f32::MAX,
            align: Alignment::new(),
            color: Color::new(0.0, 0.0, 0.0, 0.0),
            scissor: None,
        }
    }
}

/// Struct to store min and max bounds when measuring text with text_bounds or text_box_bounds
#[derive(Clone, Copy, Debug)]
pub struct TextBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl TextBounds {
    /// Creates new TextBounds struct instance from array
    fn new(bounds: &[f32; 4]) -> TextBounds {
        TextBounds {
            min_x: bounds[0],
            min_y: bounds[1],
            max_x: bounds[2],
            max_y: bounds[3],
        }
    }
}

/// Iterator over text glyph positions, calculated by Context::text_glyph_positions
pub struct TextGlyphPositions<'a> {
    context: &'a Context,
    x: f32,
    y: f32,
    start: *const c_char,
    glyphs: [ffi::NVGglyphPosition; 2],
}

impl<'a> TextGlyphPositions<'a> {
    /// Creates new TextGlyphPositions iterator with needed variables for iterating over glyphs in text
    fn new(context: &'a Context, x: f32, y: f32, text: CString) -> TextGlyphPositions<'a> {
        TextGlyphPositions {
            context: context,
            x: x,
            y: y,
            start: text.into_raw(),
            glyphs: [ffi::NVGglyphPosition {
                s: 0 as *const _,
                x: 0.0,
                minx: 0.0,
                maxx: 0.0,
            }; 2]
        }
    }
}

/// Holds computed values for given row.
#[derive(Clone, Copy, Debug)]
pub struct TextRow<'a> {
    pub width: f32,
    pub min_x: f32,
    pub max_x: f32,
    pub text: &'a str,
}

impl<'a> TextRow<'a> {
    /// Creates new TextRow from raw nanovg text row
    /// and also adds text contained in this row.
    fn new(row: &ffi::NVGtextRow, text: &'a str) -> TextRow<'a> {
        TextRow {
            width: row.width,
            min_x: row.minx,
            max_x: row.maxx,
            text: text,
        }
    }
}

/// Iterator over rows in text
/// Returned by Context::text_break_lines
#[derive(Debug)]
pub struct TextBreakLines<'a> {
    context: &'a Context,
    start: *const c_char,
    break_row_width: f32,
    row: ffi::NVGtextRow,
}

impl<'a> TextBreakLines<'a> {
    /// Creates new TextBreakLines iterator which iterated over all text rows in text.
    /// break_row_width specifies max length of row.
    fn new(context: &'a Context, text: CString, break_row_width: f32) -> TextBreakLines<'a> {
        TextBreakLines {
            context: context,
            start: text.into_raw(),
            break_row_width: break_row_width,
            row: ffi::NVGtextRow {
                start: 0 as *const _,
                end: 0 as *const _,
                next: 0 as *const _,
                width: 0.0,
                minx: 0.0,
                maxx: 0.0,
            }
        }
    }
}

impl<'a> Iterator for TextGlyphPositions<'a> {
    type Item = GlyphPosition;

    /// Returns next glyph in text
    fn next(&mut self) -> Option<Self::Item> {
        let num_glyphs = unsafe {
             ffi::nvgTextGlyphPositions(
                self.context.raw(),
                self.x,
                self.y,
                self.start,
                0 as *const _,
                self.glyphs.as_mut_ptr(),
                2
            )
        };

        match num_glyphs {
            1 => {
                self.start = &('\0' as c_char);
                Some(GlyphPosition::new(&self.glyphs[0], Box::new(None)))
            },
            2 => {
                self.x = self.glyphs[1].x;
                self.start = self.glyphs[1].s;

                Some(
                    GlyphPosition::new(
                        &self.glyphs[0],
                        Box::new(
                            Some(
                                GlyphPosition::new(
                                    &self.glyphs[1],
                                    Box::new(None)
                                )
                            )
                        )
                    )
                )
            },
            _ => None
        }
    }
}

// Stores position of glyph returned by iterator Context::text_glyph_positions
#[derive(Clone, Debug)]
pub struct GlyphPosition {
    pub x: f32,
    pub min_x: f32,
    pub max_x: f32,
    /// Next GlyphPosition for convenience (stores only one glyph position in advance)
    pub next: Box<Option<GlyphPosition>>,
}

impl GlyphPosition {
    /// Creates new GlyphPosition from raw nanovg glyph position.
    /// We can optionally pass next glyph position
    /// (there is usually some if it is not the last glyph in text, otherwise it is none for last glyph).
    fn new(glyph: &ffi::NVGglyphPosition, next: Box<Option<GlyphPosition>>) -> GlyphPosition {
        GlyphPosition {
            x: glyph.x,
            min_x: glyph.minx,
            max_x: glyph.maxx,
            next: next
        }
    }
}

/// Struct to store measured text metrics computed with Context::text_metrics
#[derive(Clone, Copy, Debug)]
pub struct TextMetrics {
    pub ascender: f32,
    pub descender: f32,
    pub line_height: f32,
}

impl TextMetrics {
    fn new() -> TextMetrics {
        TextMetrics {
            ascender: 0.0,
            descender: 0.0,
            line_height: 0.0,
        }
    }
}

impl<'a> Iterator for TextBreakLines<'a> {
    type Item = TextRow<'a>;

    /// Returns next row in text
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let nrows = ffi::nvgTextBreakLines(self.context.raw(), self.start, 0 as *const _, self.break_row_width, &mut self.row, 1);
            self.start = self.row.next;

            if nrows > 0 {
                let string_length = self.row.end as usize - self.row.start as usize;
                let string_slice = std::slice::from_raw_parts(self.row.start as *const u8, string_length);
                let text_str = std::str::from_utf8(string_slice).unwrap();
                Some(TextRow::new(&self.row, text_str))
            } else {
                None
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Alignment(ffi::NVGalign);

impl Alignment {
    fn into_raw(self) -> ffi::NVGalign {
        self.0
    }

    /// Create a new top-left alignment.
    pub fn new() -> Self {
        Alignment(ffi::NVGalign::empty()).top().left()
    }

    /// Set the horizontal alignment to left.
    pub fn left(mut self) -> Self {
        self.0.remove(ffi::NVGalign::NVG_ALIGN_RIGHT);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_CENTER);
        self.0.insert(ffi::NVGalign::NVG_ALIGN_LEFT);
        self
    }

    /// Set the horizontal alignment to center.
    pub fn center(mut self) -> Self {
        self.0.remove(ffi::NVGalign::NVG_ALIGN_LEFT);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_RIGHT);
        self.0.insert(ffi::NVGalign::NVG_ALIGN_CENTER);
        self
    }

    /// Set the horizontal alignment to right.
    pub fn right(mut self) -> Self {
        self.0.remove(ffi::NVGalign::NVG_ALIGN_LEFT);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_CENTER);
        self.0.insert(ffi::NVGalign::NVG_ALIGN_RIGHT);
        self
    }

    /// Set the vertical alignment to top.
    pub fn top(mut self) -> Self {
        self.0.remove(ffi::NVGalign::NVG_ALIGN_BOTTOM);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_MIDDLE);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_BASELINE);
        self.0.insert(ffi::NVGalign::NVG_ALIGN_TOP);
        self
    }

    /// Set the vertical alignment to middle.
    pub fn middle(mut self) -> Self {
        self.0.remove(ffi::NVGalign::NVG_ALIGN_TOP);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_BOTTOM);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_BASELINE);
        self.0.insert(ffi::NVGalign::NVG_ALIGN_MIDDLE);
        self
    }

    /// Set the vertical alignment to bottom.
    pub fn bottom(mut self) -> Self {
        self.0.remove(ffi::NVGalign::NVG_ALIGN_TOP);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_MIDDLE);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_BASELINE);
        self.0.insert(ffi::NVGalign::NVG_ALIGN_BOTTOM);
        self
    }

    /// Set the vertical alignment to baseline.
    pub fn baseline(mut self) -> Self {
        self.0.remove(ffi::NVGalign::NVG_ALIGN_TOP);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_MIDDLE);
        self.0.remove(ffi::NVGalign::NVG_ALIGN_BOTTOM);
        self.0.insert(ffi::NVGalign::NVG_ALIGN_BASELINE);
        self
    }
}

/// Represents a transformation in 2D space.
/// A transformation is a column-major matrix in the following form:
/// [a c e] - indices [0 2 4]
/// [b d f] - indices [1 3 5]
/// [0 0 1] - not stored.
/// The last row however is not stored to save some bytes; it is always [0 0 1] behind the scenes.
#[derive(Clone, Copy, Debug)]
pub struct Transform {
    pub matrix: [f32; 6],
}

impl Transform {
    /// Construct a new transform with an identity matrix.
    /// This corresponds to the origin of the coordinate system.
    pub fn new() -> Self {
        Self {
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    /// Create a new transform with a translation.
    pub fn from_translation(x: f32, y: f32) -> Self {
        let mut new = Self::new();
        new.matrix[4] = x;
        new.matrix[5] = y;
        new
    }

    /// Create a new transform with a scale.
    pub fn from_scale(x: f32, y: f32) -> Self {
        let mut new = Self::new();
        new.matrix[0] = x;
        new.matrix[3] = y;
        new
    }

    /// Create a new transform with a skew.
    pub fn from_skew(x: f32, y: f32) -> Self {
        let mut new = Self::new();
        new.matrix[2] = x;
        new.matrix[1] = y;
        new
    }

    /// Create a new transform with a rotation.
    pub fn from_rotation(angle: f32) -> Self {
        let mut new = Self::new();
        let (cos, sin) = (angle.cos(), angle.sin());
        new.matrix[0] = cos;
        new.matrix[1] = sin;
        new.matrix[2] = -sin;
        new.matrix[3] = cos;
        new
    }

    /// Convert this transform into a column-major 4x4 matrix.
    pub fn as_matrix4(&self) -> [f32; 4*4] {
        let m = &self.matrix;
        [
            m[0], m[1], m[2], 0.0,
            m[3], m[4], m[5], 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ]
    }
}

impl std::ops::Mul for Transform {
    type Output = Transform;
    fn mul(self, rhs: Transform) -> Self::Output {
        let mut res_trans = Transform { matrix: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0] };
        let m1 = &self.matrix;
        let m2 = &rhs.matrix;
        {
            let res = &mut res_trans.matrix;
            // [a c e] - indices [0 2 4]
            // [b d f] - indices [1 3 5]
            // [0 0 1] 
            res[0] = (m1[0] * m2[0]) + (m1[2] * m2[1]);
            res[2] = (m1[0] * m2[2]) + (m1[2] * m2[3]);
            res[4] = (m1[0] * m2[4]) + (m1[2] * m2[5]) + m1[4];
            
            res[1] = (m1[1] * m2[0]) + (m1[3] * m2[1]);
            res[3] = (m1[1] * m2[2]) + (m1[3] * m2[3]);
            res[5] = (m1[1] * m2[4]) + (m1[3] * m2[5]) + m1[5];
        }
        res_trans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! f32_eq {
        ($f1:expr, $f2:expr) => {
            ($f1 - $f2).abs() < std::f32::EPSILON
        };
    }

    macro_rules! trans_eq_bool {
        ($t1:expr, $t2:expr) => {
            f32_eq!($t1.matrix[0], $t2.matrix[0]) &&
            f32_eq!($t1.matrix[1], $t2.matrix[1]) &&
            f32_eq!($t1.matrix[2], $t2.matrix[2]) &&
            f32_eq!($t1.matrix[3], $t2.matrix[3]) &&
            f32_eq!($t1.matrix[4], $t2.matrix[4]) &&
            f32_eq!($t1.matrix[5], $t2.matrix[5])
        };
    }

    macro_rules! trans_eq {
        ($t1:expr, $t2:expr) => {
            assert!(trans_eq_bool!($t1, $t2))
        };
    }

    macro_rules! trans_not_eq {
        ($t1:expr, $t2:expr) => {
            assert!(!trans_eq_bool!($t1, $t2))
        };
    }

    macro_rules! mat4_eq {
        ($m1:expr, $m2:expr) => {
            let res = {
                f32_eq!($m1[0], $m2[0]) &&
                f32_eq!($m1[1], $m2[1]) &&
                f32_eq!($m1[2], $m2[2]) &&
                f32_eq!($m1[3], $m2[3]) &&
                f32_eq!($m1[4], $m2[4]) &&
                f32_eq!($m1[5], $m2[5]) &&
                f32_eq!($m1[6], $m2[6]) &&
                f32_eq!($m1[7], $m2[7]) &&
                f32_eq!($m1[8], $m2[8]) &&
                f32_eq!($m1[9], $m2[9]) &&
                f32_eq!($m1[10], $m2[10]) &&
                f32_eq!($m1[11], $m2[11]) &&
                f32_eq!($m1[12], $m2[12]) &&
                f32_eq!($m1[13], $m2[13]) &&
                f32_eq!($m1[14], $m2[14]) &&
                f32_eq!($m1[15], $m2[15])
            };
            assert!(res)
        };
    }

    #[test]
    fn test_transform() {
        // Contructors
        trans_eq!(Transform::new(), Transform {
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        });

        trans_eq!(Transform::from_translation(11.1, 22.2), Transform {
            matrix: [1.0, 0.0, 0.0, 1.0, 11.1, 22.2],
        });

        trans_eq!(Transform::from_scale(11.1, 22.2), Transform {
            matrix: [11.1, 0.0, 0.0, 22.2, 0.0, 0.0],
        });

        trans_eq!(Transform::from_skew(11.1, 22.2), Transform {
            matrix: [1.0, 22.2, 11.1, 1.0, 0.0, 0.0],
        });

        let angle = 90f32.to_radians();
        trans_eq!(Transform::from_rotation(angle), Transform {
            matrix: [angle.cos(), angle.sin(), -angle.sin(), angle.cos(), 0.0, 0.0],
        });

        // Matrix conversion
        mat4_eq!((Transform {
            matrix: [
                1.0, 3.0, 5.0,
                2.0, 4.0, 6.0,
            ],
        }).as_matrix4(), [
            1.0, 3.0, 5.0, 0.0,
            2.0, 4.0, 6.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ]);

        // Multiplication
        let identity = Transform::new();
        let trans = Transform::from_translation(10.0, 20.0);
        trans_eq!(identity * trans, trans);
        trans_eq!(trans * identity, trans);
        let a = Transform::from_rotation(123.0);
        let b = Transform::from_skew(66.6, 1337.2);
        trans_not_eq!(a * b, b * a);
    }
}