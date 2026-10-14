use super::{
    Action, Align, ChartIndex, LayerColour, LayerIndex, LayerNotes, LayerView, Layout, RichText,
    Song, SongDrafts, Ui, egui, icon, style,
};

/// Rows of the open chart's layer stack and the confirmation for deleting notes.
#[expect(clippy::too_many_lines, reason = "one row and its deletion dialog")]
#[expect(
    clippy::non_ascii_literal,
    reason = "layer status badges are separated by visible middle dots"
)]
pub(super) fn layers(ui: &mut Ui, song: &Song, drafts: &mut SongDrafts, actions: &mut Vec<Action>) {
    let rows = song.layers();
    let read_only = song.read_only().map(|why| format!("Read only: {why}"));
    let editable = read_only.is_none();
    let why = read_only.as_deref().unwrap_or_default();
    for (index, row) in rows.iter().enumerate() {
        let index = LayerIndex::new(index);
        let hover_rect = egui::Rect::from_min_size(
            ui.next_widget_position(),
            egui::vec2(ui.available_width(), 25.0),
        );
        let row_response = ui.interact(
            hover_rect,
            ui.id().with(("layer row", index.get())),
            egui::Sense::click(),
        );
        let hovered = ui.rect_contains_pointer(hover_rect);
        let fill = if song.active_layer() == index {
            style::ACCENT_FILL
        } else if hovered {
            style::ROW_HOVER
        } else {
            egui::Color32::TRANSPARENT
        };
        let mut handled = false;
        let (drag, dropped) = ui
            .scope(|ui| {
                ui.visuals_mut().widgets.inactive.bg_fill = fill;
                ui.dnd_drop_zone::<LayerIndex, _>(egui::Frame::new().corner_radius(4), |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let handle = ui
                            .add_enabled_ui(editable, |ui| {
                                ui.dnd_drag_source(
                                    ui.id().with(("layer", index.get())),
                                    index,
                                    |ui| {
                                        ui.label(
                                            style::glyph(ui, icon::DOTS_SIX_VERTICAL)
                                                .color(style::WEAK_TEXT),
                                        );
                                    },
                                )
                            })
                            .inner;
                        handled |= handle.response.clicked();
                        handle
                            .response
                            .clone()
                            .on_hover_cursor(egui::CursorIcon::Grab)
                            .on_disabled_hover_text(why);
                        handle.response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                format!("Reorder {}", row.name),
                            )
                        });
                        let (symbol, next) = match row.view {
                            LayerView::Shown => (icon::EYE, LayerView::Ghosted),
                            LayerView::Ghosted => (icon::EYE, LayerView::Hidden),
                            LayerView::Hidden => (icon::EYE_SLASH, LayerView::Shown),
                        };
                        let view_colour = match row.view {
                            LayerView::Shown => style::TEXT,
                            LayerView::Ghosted => style::TEXT.gamma_multiply(0.4),
                            LayerView::Hidden => style::WEAK_TEXT,
                        };
                        let (state, next_state) = match row.view {
                            LayerView::Shown => ("shown", "ghost"),
                            LayerView::Ghosted => ("ghosted", "hide"),
                            LayerView::Hidden => ("hidden", "show"),
                        };
                        let view = ui
                            .add_enabled(
                                editable,
                                egui::Button::new(style::glyph(ui, symbol).color(view_colour))
                                    .frame_when_inactive(false),
                            )
                            .on_hover_text(format!(
                                "{state}; click to {next_state}; Alt+click to solo"
                            ))
                            .on_disabled_hover_text(why);
                        view.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                format!("Cycle visibility of {}", row.name),
                            )
                        });
                        if view.clicked() {
                            handled = true;
                            if click_modifier(ui, &view, |modifiers| modifiers.alt) {
                                actions.push(Action::SoloLayer(index));
                            } else {
                                actions.push(Action::SetLayerView(index, next));
                            }
                        }
                        let locked = if row.locked {
                            icon::LOCK_SIMPLE
                        } else {
                            icon::LOCK_SIMPLE_OPEN
                        };
                        let lock_colour = if row.locked {
                            style::WARN
                        } else if hovered {
                            style::TEXT
                        } else {
                            style::WEAK_TEXT
                        };
                        let lock = ui
                            .add_enabled(
                                editable,
                                egui::Button::new(style::glyph(ui, locked).color(lock_colour))
                                    .frame_when_inactive(false),
                            )
                            .on_hover_text(if row.locked {
                                "locked; click to unlock"
                            } else {
                                "unlocked; click to lock"
                            })
                            .on_disabled_hover_text(why);
                        lock.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                editable,
                                row.locked,
                                format!("Lock {}", row.name),
                            )
                        });
                        if lock.clicked() {
                            handled = true;
                            actions.push(Action::SetLayerLocked(index, !row.locked));
                        }
                        let palette = ui
                            .add_enabled_ui(editable, |ui| {
                                ui.menu_button(" ", |ui| {
                                    layer_palette(ui, index, row.colour, actions);
                                })
                            })
                            .inner;
                        handled |= palette.response.clicked();
                        paint_swatch(ui, palette.response.rect.center(), row.colour, false);
                        palette.response.clone().on_disabled_hover_text(why);
                        palette.response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                format!("Colour of {}", row.name),
                            )
                        });
                        if let Some((editing, name)) = drafts.layer_name.as_mut()
                            && *editing == index
                        {
                            let width = ui.available_width().max(95.0);
                            let edit = ui
                                .add_enabled(
                                    editable,
                                    egui::TextEdit::singleline(name).desired_width(width),
                                )
                                .on_disabled_hover_text(why);
                            edit.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::TextEdit,
                                    true,
                                    format!("Rename {}", row.name),
                                )
                            });
                            if std::mem::take(&mut drafts.refocus_layer_name) {
                                edit.request_focus();
                            }
                            if !editable || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                                drafts.layer_name = None;
                            } else if ui.input(|input| input.key_pressed(egui::Key::Enter))
                                || (edit.lost_focus() && !edit.has_focus())
                            {
                                actions.push(Action::RenameLayer(index, name.clone()));
                            }
                        } else {
                            let text =
                                RichText::new(&row.name).color(if row.view == LayerView::Hidden {
                                    style::WEAK_TEXT
                                } else if row.view == LayerView::Ghosted {
                                    style::TEXT.gamma_multiply(0.55)
                                } else {
                                    style::TEXT
                                });
                            let text = if row.view == LayerView::Ghosted {
                                text.italics()
                            } else {
                                text
                            };
                            let label = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                            label.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("Layer {}", row.name),
                                )
                            });
                            if editable && label.double_clicked() {
                                drafts.layer_name = Some((index, row.name.clone()));
                            }
                            if label.clicked() && !label.double_clicked() {
                                handled = true;
                                if click_modifier(ui, &label, |modifiers| modifiers.command) {
                                    actions.push(Action::SelectLayer(index));
                                } else {
                                    actions.push(Action::SetActiveLayer(index));
                                }
                            }
                            label.context_menu(|menu_ui| {
                                layer_menu(menu_ui, song, index, drafts, actions);
                            });
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(9.0);
                            ui.weak(row.notes.to_string());
                        });
                    })
                })
            })
            .inner;
        drag.response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Layer row {}", row.name),
            )
        });
        row_response.clone().on_hover_text(format!(
            "{} · {} notes{}{}{}",
            row.name,
            row.notes,
            match row.view {
                LayerView::Shown => "",
                LayerView::Ghosted => " · ghosted",
                LayerView::Hidden => " · hidden",
            },
            if row.locked { " · locked" } else { "" },
            if song.active_layer() == index {
                " · active"
            } else {
                ""
            },
        ));
        row_response.context_menu(|menu_ui| layer_menu(menu_ui, song, index, drafts, actions));
        if row_response.clicked() && !handled {
            if click_modifier(ui, &row_response, |modifiers| modifiers.command) {
                actions.push(Action::SelectLayer(index));
            } else {
                actions.push(Action::SetActiveLayer(index));
            }
        }
        if editable
            && let Some(from) = dropped
            && *from != index
        {
            actions.push(Action::MoveLayer {
                from: *from,
                to: index,
            });
        }
    }
    if let Some(name) = drafts.deleting_layer.as_ref() {
        if let Some((index, row)) = rows
            .iter()
            .enumerate()
            .find(|(_, row)| row.name == name.as_str())
        {
            let index = LayerIndex::new(index);
            let modal =
                egui::Modal::new(egui::Id::new("delete layer notes")).show(ui.ctx(), |modal_ui| {
                    modal_ui.heading(format!("Delete {}?", row.name));
                    modal_ui.label(format!("This layer has {} notes.", row.notes));
                    modal_ui.horizontal(|buttons| {
                        if buttons
                            .button(format!("Delete {} notes", row.notes))
                            .clicked()
                        {
                            actions.push(Action::DeleteLayer(index, LayerNotes::Delete));
                            drafts.deleting_layer = None;
                        }
                        if buttons
                            .add_enabled(
                                index.get().saturating_add(1) < rows.len(),
                                egui::Button::new("Merge them down"),
                            )
                            .clicked()
                        {
                            actions.push(Action::DeleteLayer(index, LayerNotes::MergeDown));
                            drafts.deleting_layer = None;
                        }
                        if buttons.button("Cancel").clicked() {
                            drafts.deleting_layer = None;
                        }
                    });
                });
            if modal.should_close() {
                drafts.deleting_layer = None;
            }
        } else {
            drafts.deleting_layer = None;
        }
    }
}

/// Read click modifiers before egui's queued input resets them.
fn click_modifier(
    ui: &Ui,
    response: &egui::Response,
    wanted: impl Fn(egui::Modifiers) -> bool,
) -> bool {
    ui.input(|input| input.events.iter().any(|event| {
        matches!(event, egui::Event::PointerButton { pos, pressed: false, button: egui::PointerButton::Primary, modifiers } if response.rect.contains(*pos) && wanted(*modifiers))
    }))
}

/// Eight fixed choices shared by each layer's swatch and menu.
fn paint_swatch(ui: &Ui, centre: egui::Pos2, colour: LayerColour, selected: bool) {
    let square = egui::Rect::from_center_size(centre, egui::vec2(12.0, 12.0));
    ui.painter()
        .rect_filled(square, 3.0, style::layer_colour(colour));
    ui.painter().rect_stroke(
        square,
        3.0,
        egui::Stroke::new(1.0, style::TEXT.gamma_multiply(0.55)),
        egui::StrokeKind::Outside,
    );
    if selected {
        ui.painter().rect_stroke(
            square.expand(3.0),
            5.0,
            egui::Stroke::new(1.5, style::ACCENT),
            egui::StrokeKind::Outside,
        );
    }
}

/// Eight named swatches arranged in two rows.
fn layer_palette(ui: &mut Ui, index: LayerIndex, current: LayerColour, actions: &mut Vec<Action>) {
    egui::Grid::new(("layer palette", index.get()))
        .num_columns(4)
        .show(ui, |ui| {
            for (i, colour) in LayerColour::ALL.into_iter().enumerate() {
                let name = format!("{colour:?}");
                let response = ui.add(egui::Button::new(" ").min_size(egui::vec2(24.0, 24.0)));
                paint_swatch(ui, response.rect.center(), colour, colour == current);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name)
                });
                if response.on_hover_text(&name).clicked() {
                    actions.push(Action::RecolourLayer(index, colour));
                    ui.close();
                }
                if i == 3 {
                    ui.end_row();
                }
            }
        });
}

/// Commands for one layer; the header menu addresses the active row.
pub(super) fn layer_menu(
    ui: &mut Ui,
    song: &Song,
    index: LayerIndex,
    drafts: &mut SongDrafts,
    actions: &mut Vec<Action>,
) {
    let rows = song.layers();
    let Some(row) = rows.get(index.get()) else {
        return;
    };
    let read_only = song.read_only().map(|why| format!("Read only: {why}"));
    let editable = read_only.is_none();
    let why = read_only.as_deref().unwrap_or_default();
    let edit_button = |item_ui: &mut Ui, label: &str, available: bool| {
        item_ui
            .add_enabled(editable && available, egui::Button::new(label))
            .on_disabled_hover_text(why)
    };
    if edit_button(ui, "New layer above", true).clicked() {
        actions.push(Action::SetActiveLayer(index));
        actions.push(Action::AddLayer);
        ui.close();
    }
    ui.separator();
    if edit_button(ui, "Rename", true).clicked() {
        drafts.layer_name = Some((index, row.name.clone()));
        ui.close();
    }
    if editable {
        ui.menu_button("Colour", |ui| layer_palette(ui, index, row.colour, actions));
    } else {
        edit_button(ui, "Colour", true);
    }
    ui.separator();
    for (label, view) in [
        ("Show", LayerView::Shown),
        ("Ghost", LayerView::Ghosted),
        ("Hide", LayerView::Hidden),
    ] {
        if ui
            .add_enabled_ui(editable, |ui| ui.radio(row.view == view, label))
            .inner
            .on_disabled_hover_text(why)
            .clicked()
        {
            actions.push(Action::SetLayerView(index, view));
            ui.close();
        }
    }
    if edit_button(ui, if row.locked { "Unlock" } else { "Lock" }, true).clicked() {
        actions.push(Action::SetLayerLocked(index, !row.locked));
        ui.close();
    }
    if edit_button(ui, "Solo", true).clicked() {
        actions.push(Action::SoloLayer(index));
        ui.close();
    }
    ui.separator();
    if ui.button("Select notes").clicked() {
        actions.push(Action::SelectLayer(index));
        ui.close();
    }
    let has_selection = song.selection().next().is_some();
    if edit_button(ui, "Move selected notes here", has_selection).clicked() {
        actions.push(Action::MoveSelectionToLayer(index));
        ui.close();
    }
    if edit_button(ui, "Merge down", index.get().saturating_add(1) < rows.len()).clicked() {
        actions.push(Action::MergeLayer(index));
        ui.close();
    }
    if editable {
        ui.menu_button("Copy to chart", |ui| {
            for (chart, target) in song.document().document().charts.iter().enumerate() {
                let chart = ChartIndex::new(chart);
                if chart != song.chart_index()
                    && ui.button(format!("Copy to {}", target.name)).clicked()
                {
                    actions.push(Action::CopyLayerToChart {
                        layer: index,
                        chart,
                    });
                    ui.close();
                }
            }
        });
    } else {
        edit_button(ui, "Copy to chart", true);
    }
    ui.separator();
    if edit_button(ui, "Delete", rows.len() > 1).clicked() {
        if row.notes == 0 {
            actions.push(Action::DeleteLayer(index, LayerNotes::Delete));
        } else {
            drafts.deleting_layer = song.chart().and_then(|chart| {
                chart
                    .layers
                    .get(index.get())
                    .map(|layer| layer.name.clone())
            });
        }
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use egui::accesskit::{Role, Toggled};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::*;

    fn editable_layers() -> Harness<'static, (Song, SongDrafts, Vec<Action>)> {
        let song = Song::new(charter_editor::demo::song(), &[&charter_stepmania::Plugin]);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(650.0, 900.0))
            .build_ui_state(
                |ui, (song, drafts, actions): &mut (Song, SongDrafts, Vec<Action>)| {
                    super::super::song::song_panel(ui, song, (drafts, None), actions);
                },
                (song, SongDrafts::default(), Vec::new()),
            );
        harness.run();
        harness
    }

    fn read_only_layers() -> Harness<'static, (Song, SongDrafts, Vec<Action>)> {
        let mut harness = editable_layers();
        harness.state_mut().0.add_layer().unwrap();
        let document = harness.state().0.document().clone();
        harness.state_mut().0 = Song::new(document, &[]);
        harness.run();
        assert!(harness.state().0.read_only().is_some());
        harness
    }

    #[test]
    fn the_lock_button_reports_its_state_under_one_name() {
        let mut harness = editable_layers();
        assert_eq!(
            harness
                .query_by_label("Lock Layer 0")
                .map(|node| node.accesskit_node().toggled()),
            Some(Some(Toggled::False))
        );
        harness
            .state_mut()
            .0
            .set_layer_locked(LayerIndex::new(0), true)
            .unwrap();
        harness.run();
        assert_eq!(
            harness
                .query_by_label("Lock Layer 0")
                .map(|node| node.accesskit_node().toggled()),
            Some(Some(Toggled::True))
        );
    }

    #[test]
    fn a_read_only_layer_disables_changes_but_can_still_be_selected() {
        let mut harness = read_only_layers();
        for label in [
            "Reorder Layer 0",
            "Cycle visibility of Layer 0",
            "Lock Layer 0",
            "Colour of Layer 0",
        ] {
            assert!(
                harness.get_by_label(label).accesskit_node().is_disabled(),
                "{label}"
            );
        }
        assert!(
            !harness
                .get_by_label("Layer Layer 0")
                .accesskit_node()
                .is_disabled()
        );
        harness
            .ctx
            .options_mut(|options| options.input_options.max_double_click_delay = 2.0);
        harness.get_by_label("Layer Layer 0").click();
        harness.step();
        harness.get_by_label("Layer Layer 0").click();
        harness.step();
        harness.run();
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "Rename Layer 0")
                .is_none()
        );
    }

    #[test]
    fn a_read_only_layer_menu_disables_edits_but_offers_selection() {
        let mut harness = read_only_layers();
        harness.state_mut().0.select_all();
        harness.run();
        harness.get_by_label("Layer row Layer 1").click_secondary();
        harness.run();
        for label in [
            "New layer above",
            "Rename",
            "Colour",
            "Show",
            "Ghost",
            "Hide",
            "Lock",
            "Solo",
            "Move selected notes here",
            "Merge down",
            "Copy to chart",
            "Delete",
        ] {
            assert!(
                harness.get_by_label(label).accesskit_node().is_disabled(),
                "{label}"
            );
        }
        assert!(
            !harness
                .get_by_label("Select notes")
                .accesskit_node()
                .is_disabled()
        );
        harness.get_by_label("Select notes").click();
        harness.run();
        assert_eq!(harness.state().2, [Action::SelectLayer(LayerIndex::new(0))]);
    }

    fn ask_to_delete_layer_zero(harness: &mut Harness<'_, (Song, SongDrafts, Vec<Action>)>) {
        harness.get_by_label("Layer Layer 0").click_secondary();
        harness.run();
        harness.get_by_label("Delete").click();
        harness.run();
        assert!(harness.query_by_label("Delete Layer 0?").is_some());
    }

    #[test]
    fn confirming_a_layer_deletion_after_reordering_targets_the_same_layer() {
        let mut harness = editable_layers();
        harness.state_mut().0.add_layer().unwrap();
        harness.run();
        ask_to_delete_layer_zero(&mut harness);

        harness
            .state_mut()
            .0
            .move_layer(LayerIndex::new(1), LayerIndex::new(0))
            .unwrap();
        harness.run();
        assert!(harness.query_by_label("Delete Layer 0?").is_some());
        harness.get_by_label("Delete 124 notes").click();
        harness.run();
        assert_eq!(
            harness.state().2,
            [Action::DeleteLayer(LayerIndex::new(0), LayerNotes::Delete)]
        );
    }

    #[test]
    fn deleting_a_layer_while_its_question_is_open_drops_the_question() {
        let mut harness = editable_layers();
        harness.state_mut().0.add_layer().unwrap();
        harness.state_mut().0.add_layer().unwrap();
        harness
            .state_mut()
            .0
            .move_layer(LayerIndex::new(2), LayerIndex::new(0))
            .unwrap();
        harness.run();
        ask_to_delete_layer_zero(&mut harness);

        harness
            .state_mut()
            .0
            .delete_layer(LayerIndex::new(0), LayerNotes::Delete)
            .unwrap();
        harness.run();
        assert!(harness.state().1.deleting_layer.is_none());
        assert!(harness.query_by_label("Delete Layer 2?").is_none());
        assert!(harness.state().2.is_empty());
    }
}
