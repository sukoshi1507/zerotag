// SPDX-License-Identifier: MIT
//! The Famicom palette (dark only) and the layout constants of the screens (same values as the C++ UI).

use crate::canvas::Rgb;

pub const BACKGROUND: Rgb = 0x101014;
pub const SURFACE: Rgb = 0x1a1518;
pub const BAR: Rgb = 0x0b0b10;
pub const BUTTON: Rgb = 0x0b0b10;
pub const BORDER: Rgb = 0x4a3a3c;
pub const TEXT: Rgb = 0xf1ebd8;
pub const CREAM: Rgb = 0xece6d3;
pub const DARK_RED: Rgb = 0x6b2a30;
pub const ACCENT: Rgb = 0xc8202f;
pub const PRIMARY: Rgb = 0xf3c93a;
pub const WARNING: Rgb = 0xf3c93a;
pub const ERROR: Rgb = 0xe88080;
pub const DIM: Rgb = 0xa79f8e;
pub const SELECTED_ROW: Rgb = 0x2a1417;

pub const INSET: i32 = 4; // width of the pixel frame
pub const BAR_H: i32 = 18;
pub const FOOT_H: i32 = 18;
pub const INNER_W: i32 = crate::SCREEN_W - 2 * INSET; // 312
pub const CONTENT_TOP: i32 = INSET + BAR_H; // 22
pub const FOOT_TOP: i32 = crate::SCREEN_H - INSET - FOOT_H; // 148
pub const LIST_W: i32 = 190;
pub const ROW_H: i32 = 26;
pub const LIST_TOP: i32 = 44; // below the search field
pub const ROWS: i32 = 4;
