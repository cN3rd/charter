use super::{
    Action, Align, LayerColour, LayerNotes, LayerView, Layout, RichText, Song, SongDrafts, Ui,
    egui, icon, style,
};

/// Rows of the open chart's layer stack and the confirmation for deleting notes.
#[expect(clippy::too_many_lines, reason = "one row and its deletion dialog")]
#[expect(
    clippy::non_ascii_literal,
    reason = "layer status badges are separated by visible middle dots"
)]
pub(super) fn layers(ui: &mut Ui, song: &Song, drafts: &mut SongDrafts, actions: &mut Vec<Action>) {
    let rows = song.layers();
    for (index, row) in rows.iter().enumerate() {
        let hover_rect = egui::Rect::from_min_size(
            ui.next_widget_position(),
            egui::vec2(ui.available_width(), 25.0),
        );
        let row_response = ui.interact(
            hover_rect,
            ui.id().with(("layer row", index)),
            egui::Sense::click(),
        );
        let hovered = ui.rect_contains_pointer(hover_rect);
        let fill = if song.active_layer() == index {
            style::ACCENT_FILL
        } else if hovered {
            egui::Color32::from_gray(40)
        } else {
            egui::Color32::TRANSPARENT
        };
        let mut handled = false;
        let (drag, dropped) = ui
            .scope(|ui| {
                ui.visuals_mut().widgets.inactive.bg_fill = fill;
                ui.dnd_drop_zone::<usize, _>(egui::Frame::new().corner_radius(4), |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let handle =
                            ui.dnd_drag_source(ui.id().with(("layer", index)), index, |ui| {
                                ui.label(
                                    style::glyph(ui, icon::DOTS_SIX_VERTICAL)
                                        .color(style::WEAK_TEXT),
                                );
                            });
                        handled |= handle.response.clicked();
                        handle
                            .response
                            .clone()
                            .on_hover_cursor(egui::CursorIcon::Grab);
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
                            .add(
                                egui::Button::new(style::glyph(ui, symbol).color(view_colour))
                                    .frame_when_inactive(false),
                            )
                            .on_hover_text(format!(
                                "{state}; click to {next_state}; Alt+click to solo"
                            ));
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
                            style::WEAK_TEXT.gamma_multiply(0.55)
                        };
                        let lock = ui
                            .add(
                                egui::Button::new(style::glyph(ui, locked).color(lock_colour))
                                    .frame_when_inactive(false),
                            )
                            .on_hover_text(if row.locked {
                                "locked; click to unlock"
                            } else {
                                "unlocked; click to lock"
                            });
                        lock.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                format!("Toggle lock of {}", row.name),
                            )
                        });
                        if lock.clicked() {
                            handled = true;
                            actions.push(Action::SetLayerLocked(index, !row.locked));
                        }
                        let palette =
                            ui.menu_button(" ", |ui| layer_palette(ui, index, row.colour, actions));
                        handled |= palette.response.clicked();
                        paint_swatch(ui, palette.response.rect.center(), row.colour, false);
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
                            let edit = ui.add(egui::TextEdit::singleline(name).desired_width(95.0));
                            edit.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::TextEdit,
                                    true,
                                    format!("Rename {}", row.name),
                                )
                            });
                            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                                drafts.layer_name = None;
                            } else if ui.input(|input| input.key_pressed(egui::Key::Enter))
                                || (edit.lost_focus() && !edit.has_focus())
                            {
                                actions.push(Action::RenameLayer(index, name.clone()));
                                drafts.layer_name = None;
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
                            if label.double_clicked() {
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
        if let Some(from) = dropped
            && *from != index
        {
            actions.push(Action::MoveLayer(*from, index));
        }
    }
    if let Some(index) = drafts.deleting_layer {
        if let Some(row) = rows.get(index) {
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
                                index.saturating_add(1) < rows.len(),
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
fn layer_palette(ui: &mut Ui, index: usize, current: LayerColour, actions: &mut Vec<Action>) {
    egui::Grid::new(("layer palette", index))
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
    index: usize,
    drafts: &mut SongDrafts,
    actions: &mut Vec<Action>,
) {
    let rows = song.layers();
    let Some(row) = rows.get(index) else { return };
    if ui.button("New layer above").clicked() {
        actions.push(Action::SetActiveLayer(index));
        actions.push(Action::AddLayer);
        ui.close();
    }
    ui.separator();
    if ui.button("Rename").clicked() {
        drafts.layer_name = Some((index, row.name.clone()));
        ui.close();
    }
    ui.menu_button("Colour", |ui| layer_palette(ui, index, row.colour, actions));
    ui.separator();
    for (label, view) in [
        ("Show", LayerView::Shown),
        ("Ghost", LayerView::Ghosted),
        ("Hide", LayerView::Hidden),
    ] {
        if ui.radio(row.view == view, label).clicked() {
            actions.push(Action::SetLayerView(index, view));
            ui.close();
        }
    }
    if ui
        .button(if row.locked { "Unlock" } else { "Lock" })
        .clicked()
    {
        actions.push(Action::SetLayerLocked(index, !row.locked));
        ui.close();
    }
    if ui.button("Solo").clicked() {
        actions.push(Action::SoloLayer(index));
        ui.close();
    }
    ui.separator();
    if ui.button("Select notes").clicked() {
        actions.push(Action::SelectLayer(index));
        ui.close();
    }
    if ui
        .add_enabled(
            song.selection().next().is_some(),
            egui::Button::new("Move selected notes here"),
        )
        .clicked()
    {
        actions.push(Action::MoveSelectionToLayer(index));
        ui.close();
    }
    if ui
        .add_enabled(
            index.saturating_add(1) < rows.len(),
            egui::Button::new("Merge down"),
        )
        .clicked()
    {
        actions.push(Action::MergeLayer(index));
        ui.close();
    }
    ui.menu_button("Copy to chart", |ui| {
        for (chart, target) in song.document().document().charts.iter().enumerate() {
            if chart != song.chart_index()
                && ui.button(format!("Copy to {}", target.name)).clicked()
            {
                actions.push(Action::CopyLayerToChart(index, chart));
                ui.close();
            }
        }
    });
    ui.separator();
    if ui
        .add_enabled(rows.len() > 1, egui::Button::new("Delete"))
        .clicked()
    {
        if row.notes == 0 {
            actions.push(Action::DeleteLayer(index, LayerNotes::Delete));
        } else {
            drafts.deleting_layer = Some(index);
        }
        ui.close();
    }
}
