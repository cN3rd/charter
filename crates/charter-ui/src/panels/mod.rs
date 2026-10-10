#![expect(clippy::mod_module_files, reason = "requested directory layout")]
//! Panels around the notefield.
//!
//! The old single file is split by panel family: menus, shortcut help, song metadata and
//! layers, markers, shared text entries, transport/status controls, empty-state notices and
//! modal dialogs. Re-exports preserve the existing `crate::panels::...` paths.

use std::num::NonZeroU16;

use charter_core::{
    LayerColour, LayerView, MarkerKind, Metadata, ResolvedTime, ResourcePath, SettingScope, Tick,
};
use charter_editor::{ChartIndex, LayerIndex, LayerNotes, Song, Step};
use egui::{self, Align, Layout, RichText, Ui};
use serde_json::Value;

use crate::Action;
use crate::keys::{HELP, NEW_SONG, OPEN, REDO, SAVE, SAVE_AS, SETTINGS, UNDO};
use crate::notefield::Zoom;
use crate::style::{self, fields, heading, icon, table};
use crate::window::Rate;

/// The About window.
mod about;
/// Modal questions, toasts and read-only notices.
mod dialogs;
/// Empty-song and missing-audio centre panels.
mod empty;
/// Shared editable text entry helpers.
mod entry;
/// Chart layer stack controls.
mod layers;
/// Marker list panel.
mod markers;
/// Main menu bar and recent-file menu.
mod menu;
/// Keyboard shortcut help window.
mod shortcuts;
/// Song metadata, chart settings and details panels.
mod song;
/// Transport controls, status bar and snap labels.
mod transport;

pub(crate) use about::about;
pub(crate) use dialogs::{
    Answer, ask_to_download_weights, ask_to_reset_timing, ask_to_restore, ask_to_run_on_cpu,
    ask_to_save, choose_format, offer_tags, read_only, toast,
};
pub(crate) use empty::{empty, no_audio};
pub(crate) use entry::entry;
pub(crate) use markers::markers;
pub(crate) use menu::menu_bar;
pub(crate) use shortcuts::shortcuts;
pub(crate) use song::{SongDrafts, details, song_panel};
pub(crate) use transport::{ordinal, status_bar, transport_bar};

#[cfg(test)]
use menu::open_recent;
