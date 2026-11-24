// SPDX-License-Identifier: MIT
//! Software renderer of the ZeroTag screens (320x170, Famicom look). No GUI toolkit: a pixel canvas, Noto text,
//! and the widgets of the original design, drawn with the same coordinates as the C++ (LVGL) version.

pub mod canvas;
pub mod font;
pub mod i18n;
mod i18n_strings;
pub mod screens;
pub mod style;
pub mod thumbs;
pub mod widgets;

pub use canvas::{Canvas, Rgb};
pub use font::Fonts;
pub use i18n::Lang;
pub use i18n_strings::Str;

pub const SCREEN_W: i32 = 320;
pub const SCREEN_H: i32 = 170;
