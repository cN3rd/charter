//! Layer rows and layer-aware edits.

use super::notes::{note_at as object_note_at, taken as note_taken};
#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "song submodules share one private implementation vocabulary"
    )
)]
use super::*;

/// Lock state requested for a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayerLock {
    /// Notes on the layer cannot be edited.
    Locked,
    /// Notes on the layer can be edited when the layer is shown.
    Unlocked,
}

impl LayerLock {
    /// Stored boolean representation in the chart format.
    const fn is_locked(self) -> bool {
        matches!(self, Self::Locked)
    }
}

impl From<bool> for LayerLock {
    fn from(locked: bool) -> Self {
        if locked { Self::Locked } else { Self::Unlocked }
    }
}

/// Adds a default layer to legacy charts that were saved before layers existed.
pub(super) fn with_layers(document: ValidDocument) -> ValidDocument {
    let unlayered = document
        .document()
        .charts
        .iter()
        .any(|chart| chart.layers.is_empty());
    let Ok(name) = LayerName::new("Layer 0".to_owned()) else {
        return document;
    };
    if !unlayered {
        return document;
    }
    let mut migrated = document.document().clone();
    for chart in migrated
        .charts
        .iter_mut()
        .filter(|chart| chart.layers.is_empty())
    {
        chart.layers.push(Layer {
            name: name.clone(),
            colour: LayerColour::Blue,
            view: LayerView::Shown,
            locked: false,
        });
        for object in &mut chart.objects {
            object.layer = Some(name.clone());
        }
    }
    ValidDocument::new(migrated).unwrap_or(document)
}

impl Song {
    /// Rows of the open chart.
    #[must_use]
    pub fn layers(&self) -> Vec<LayerRow> {
        let Some(chart) = self.document.document().charts.get(self.chart) else {
            return Vec::new();
        };
        if chart.layers.is_empty() {
            return vec![LayerRow {
                name: "Notes".into(),
                colour: LayerColour::Blue,
                view: LayerView::Shown,
                locked: false,
                notes: chart.objects.len(),
            }];
        }
        let mut notes: Vec<usize> = vec![0; chart.layers.len()];
        for object in &chart.objects {
            if let Some(count) = chart
                .layer_index(object)
                .and_then(|index| notes.get_mut(index))
            {
                *count = (*count).saturating_add(1);
            }
        }
        chart
            .layers
            .iter()
            .enumerate()
            .map(|(index, layer)| LayerRow {
                name: layer.name.to_string(),
                colour: layer.colour,
                view: layer.view,
                locked: layer.locked,
                notes: notes.get(index).copied().unwrap_or(0),
            })
            .collect()
    }

    /// Number of rows in the open chart, including the implicit legacy row.
    fn layer_count(&self) -> usize {
        self.document
            .document()
            .charts
            .get(self.chart)
            .map_or(0, |chart| chart.layers.len().max(1))
    }

    /// Active row of the open chart, clamped to a row that exists.
    #[must_use]
    pub fn active_layer(&self) -> LayerIndex {
        self.chart_state
            .get(self.chart)
            .map_or(LayerIndex::ZERO, |state| state.active_layer)
            .clamped(self.layer_count())
    }

    /// Choose the row where subsequently placed notes go; out-of-range indices clamp.
    pub fn set_active_layer(&mut self, index: LayerIndex) {
        let len = self.layer_count();
        if let Some(state) = self.chart_state.get_mut(self.chart) {
            state.active_layer = index.clamped(len);
        }
    }

    /// Appearance of an object's layer; only multiple real layers carry a colour bar.
    #[must_use]
    pub fn layer_look(&self, object: &Object) -> (LayerView, Option<LayerColour>) {
        let Some(chart) = self.document.document().charts.get(self.chart) else {
            return (LayerView::Shown, None);
        };
        let Some(index) = chart.layer_index(object) else {
            return (LayerView::Shown, None);
        };
        chart
            .layers
            .get(index)
            .map_or((LayerView::Shown, None), |layer| {
                (layer.view, (chart.layers.len() > 1).then_some(layer.colour))
            })
    }

    /// Make a layer above the active row.
    ///
    /// # Errors
    /// Refuses when the song is read-only or its plugin rejects the edit.
    pub fn add_layer(&mut self) -> Result<(), Refusal> {
        let active = self.active_layer();
        self.edit_layers(|layers, _| {
            let mut n = 1_u32;
            let name = loop {
                let name = format!("Layer {n}");
                if !layers.iter().any(|layer| layer.name.as_str() == name) {
                    break name;
                }
                n = n
                    .checked_add(1)
                    .ok_or_else(|| Refusal::LayerName("no available layer name".into()))?;
            };
            let colour = LayerColour::next_unused(layers.iter().map(|layer| layer.colour));
            layers.insert(
                active.get(),
                Layer {
                    name: LayerName::new(name)
                        .map_err(|error| Refusal::LayerName(error.to_string()))?,
                    colour,
                    view: LayerView::Shown,
                    locked: false,
                },
            );
            Ok(())
        })?;
        self.set_active_layer(active);
        Ok(())
    }

    /// Rename a row and all its notes in one undoable edit.
    ///
    /// # Errors
    /// Refuses invalid or duplicate names, read-only songs and plugin rejections.
    pub fn rename_layer(&mut self, index: LayerIndex, name: &str) -> Result<(), Refusal> {
        let index = index.get();
        let name = LayerName::new(name).map_err(|error| Refusal::LayerName(error.to_string()))?;
        if self
            .layers()
            .get(index)
            .is_some_and(|row| row.name == name.as_str())
        {
            return Ok(());
        }
        self.edit_layers(|layers, objects| {
            if layers
                .iter()
                .enumerate()
                .any(|(i, layer)| i != index && layer.name == name)
            {
                return Err(Refusal::LayerName(name.to_string()));
            }
            let layer = layers
                .get_mut(index)
                .ok_or_else(|| Refusal::LayerName("no such layer".into()))?;
            let old = std::mem::replace(&mut layer.name, name.clone());
            for object in objects {
                if object.layer.as_ref() == Some(&old) {
                    object.layer = Some(name.clone());
                }
            }
            Ok(())
        })
    }

    /// Set a row's palette colour.
    ///
    /// # Errors
    /// Refuses a missing row, read-only song or plugin rejection.
    pub fn recolour_layer(
        &mut self,
        index: LayerIndex,
        colour: LayerColour,
    ) -> Result<(), Refusal> {
        let index = index.get();
        if self
            .layers()
            .get(index)
            .is_some_and(|row| row.colour == colour)
        {
            return Ok(());
        }
        self.edit_layers(|layers, _| {
            layers
                .get_mut(index)
                .ok_or_else(|| Refusal::LayerName("no such layer".into()))?
                .colour = colour;
            Ok(())
        })
    }

    /// Show, ghost or hide a row; a manual view edit forgets any solo.
    ///
    /// # Errors
    /// Refuses a missing row, read-only song or plugin rejection.
    pub fn set_layer_view(&mut self, index: LayerIndex, view: LayerView) -> Result<(), Refusal> {
        let index = index.get();
        if self.layers().get(index).is_some_and(|row| row.view == view) {
            if let Some(state) = self.chart_state.get_mut(self.chart) {
                state.solo = None;
            }
            return Ok(());
        }
        self.edit_layers(|layers, _| {
            layers
                .get_mut(index)
                .ok_or_else(|| Refusal::LayerName("no such layer".into()))?
                .view = view;
            Ok(())
        })?;
        if let Some(state) = self.chart_state.get_mut(self.chart) {
            state.solo = None;
        }
        Ok(())
    }

    /// Lock or unlock a row.
    ///
    /// # Errors
    /// Refuses a missing row, read-only song or plugin rejection.
    pub fn set_layer_locked(&mut self, index: LayerIndex, locked: bool) -> Result<(), Refusal> {
        self.set_layer_lock(index, LayerLock::from(locked))
    }

    /// Lock or unlock a row through typed internals.
    fn set_layer_lock(&mut self, index: LayerIndex, lock: LayerLock) -> Result<(), Refusal> {
        let index = index.get();
        let locked = lock.is_locked();
        if self
            .layers()
            .get(index)
            .is_some_and(|row| row.locked == locked)
        {
            return Ok(());
        }
        self.edit_layers(|layers, _| {
            layers
                .get_mut(index)
                .ok_or_else(|| Refusal::LayerName("no such layer".into()))?
                .locked = locked;
            Ok(())
        })
    }

    /// Reorder a row, keeping each note on its named layer and the active row following its layer.
    ///
    /// # Errors
    /// Refuses a missing row, read-only song or plugin rejection.
    pub fn move_layer(&mut self, from: LayerIndex, to: LayerIndex) -> Result<(), Refusal> {
        let (from, to) = (from.get(), to.get());
        if from == to && from < self.layer_count() {
            return Ok(());
        }
        let active = self.active_layer().get();
        self.edit_layers(|layers, _| {
            if from >= layers.len() || to >= layers.len() {
                return Err(Refusal::LayerName("no such layer".into()));
            }
            let layer = layers.remove(from);
            layers.insert(to, layer);
            Ok(())
        })?;
        self.set_active_layer(LayerIndex::new(if active == from {
            to
        } else if from < active && active <= to {
            active.saturating_sub(1)
        } else if to <= active && active < from {
            active.saturating_add(1)
        } else {
            active
        }));
        if let Some(state) = self.chart_state.get_mut(self.chart) {
            state.solo = None;
        }
        Ok(())
    }

    /// Move a row's notes into the next row and remove the source.
    ///
    /// # Errors
    /// Refuses the last row, read-only song or plugin rejection.
    pub fn merge_layer_down(&mut self, index: LayerIndex) -> Result<(), Refusal> {
        self.delete_layer(index, LayerNotes::MergeDown)
    }

    /// Remove a row, either deleting its notes or moving them into the next row.
    ///
    /// # Errors
    /// Refuses the only row, a missing row, read-only song or plugin rejection.
    pub fn delete_layer(&mut self, index: LayerIndex, notes: LayerNotes) -> Result<(), Refusal> {
        let index = index.get();
        // Read before the row goes, which would clamp the last row onto the one above it.
        let active = self.active_layer().get();
        self.edit_layers(|layers, objects| {
            if layers.len() <= 1 {
                return Err(Refusal::LastLayer);
            }
            let Some(layer) = layers.get(index) else {
                return Err(Refusal::LayerName("no such layer".into()));
            };
            let name = layer.name.clone();
            let into = match notes {
                LayerNotes::Delete => None,
                LayerNotes::MergeDown => Some(
                    layers
                        .get(index.saturating_add(1))
                        .ok_or(Refusal::LastLayer)?
                        .name
                        .clone(),
                ),
            };
            match into {
                Some(into) => {
                    for object in objects {
                        if object.layer.as_ref() == Some(&name) {
                            object.layer = Some(into.clone());
                        }
                    }
                }
                None => objects.retain(|object| object.layer.as_ref() != Some(&name)),
            }
            layers.remove(index);
            Ok(())
        })?;
        self.set_active_layer(LayerIndex::new(if active > index {
            active.saturating_sub(1)
        } else {
            active
        }));
        if let Some(state) = self.chart_state.get_mut(self.chart) {
            state.solo = None;
        }
        Ok(())
    }

    /// Solo a row by showing it and ghosting other shown rows, or restore the views remembered
    /// when that row was soloed.
    ///
    /// # Errors
    /// Refuses a missing row, read-only song or plugin rejection.
    pub fn solo_layer(&mut self, index: LayerIndex) -> Result<(), Refusal> {
        let index = index.get();
        let previous = self
            .chart_state
            .get(self.chart)
            .and_then(|state| state.solo.clone());
        let views: Vec<_> = self.layers().iter().map(|row| row.view).collect();
        let restoring = previous.as_ref().filter(|solo| solo.layer.get() == index);
        self.edit_layers(|layers, _| {
            if index >= layers.len() {
                return Err(Refusal::LayerName("no such layer".into()));
            }
            if let Some(solo) = restoring {
                for (layer, view) in layers.iter_mut().zip(&solo.views) {
                    layer.view = *view;
                }
            } else {
                for (i, layer) in layers.iter_mut().enumerate() {
                    match (i == index, layer.view) {
                        (true, _) => layer.view = LayerView::Shown,
                        (false, LayerView::Shown) => layer.view = LayerView::Ghosted,
                        (false, LayerView::Ghosted | LayerView::Hidden) => {}
                    }
                }
            }
            Ok(())
        })?;
        if let Some(state) = self.chart_state.get_mut(self.chart) {
            state.solo = if restoring.is_some() {
                None
            } else {
                Some(SoloLayer {
                    layer: LayerIndex::new(index),
                    views,
                })
            };
        }
        Ok(())
    }

    /// Move selected notes to another editable row, preserving their ticks and positions.
    ///
    /// # Errors
    /// Refuses protected destinations, read-only songs and plugin rejections.
    pub fn move_selection_to_layer(&mut self, index: LayerIndex) -> Result<(), Refusal> {
        let index = index.get();
        let name = self.editable_layer(index)?.cloned();
        if self.selection.is_empty()
            || self
                .document
                .document()
                .charts
                .get(self.chart)
                .is_some_and(|chart| {
                    chart
                        .objects
                        .iter()
                        .filter(|object| self.is_selected(object))
                        .all(|object| chart.layer_index(object).unwrap_or(0) == index)
                })
        {
            return Ok(());
        }
        let selection = self.selection.clone();
        self.edit_layers(|_, objects| {
            for object in objects {
                if object_note_at(object).is_some_and(|note| selection.contains(&note)) {
                    object.layer.clone_from(&name);
                }
            }
            Ok(())
        })
    }

    /// Select notes in an editable row; protected rows select nothing.
    pub fn select_layer(&mut self, index: LayerIndex) {
        let index = index.get();
        let found = self
            .document
            .document()
            .charts
            .get(self.chart)
            .map(|chart| {
                chart
                    .objects
                    .iter()
                    .filter(|object| {
                        chart.layer_index(object).unwrap_or(0) == index && self.is_editable(object)
                    })
                    .filter_map(object_note_at)
                    .collect()
            })
            .unwrap_or_default();
        self.selection = found;
    }

    /// Copy one layer, its view, lock state and notes into another chart, skipping occupied note
    /// spots. Returns the number of notes skipped.
    ///
    /// # Errors
    /// Refuses missing charts or rows, read-only songs and plugin rejections.
    pub fn copy_layer_to_chart(
        &mut self,
        index: LayerIndex,
        chart: ChartIndex,
    ) -> Result<usize, Refusal> {
        let index = index.get();
        let chart = chart.get();
        if self.plugin().is_none() {
            return Err(Refusal::ReadOnly);
        }
        let source = self
            .document
            .document()
            .charts
            .get(self.chart)
            .ok_or(Refusal::NoChart)?;
        let row = self
            .layers()
            .get(index)
            .cloned()
            .ok_or_else(|| Refusal::LayerName("no such layer".into()))?;
        let layer = Layer {
            name: LayerName::new(&row.name)
                .map_err(|error| Refusal::LayerName(error.to_string()))?,
            colour: row.colour,
            view: row.view,
            locked: row.locked,
        };
        let copied: Vec<_> = source
            .objects
            .iter()
            .filter(|object| source.layer_index(object).unwrap_or(0) == index)
            .cloned()
            .collect();
        let target = self
            .document
            .document()
            .charts
            .get(chart)
            .ok_or(Refusal::NoChart)?;
        let mut layers = target.layers.clone();
        let mut objects = target.objects.clone();
        if layers.is_empty() {
            let notes =
                LayerName::new("Notes").map_err(|error| Refusal::LayerName(error.to_string()))?;
            layers.push(Layer {
                name: notes,
                colour: LayerColour::Blue,
                view: LayerView::Shown,
                locked: false,
            });
        }
        if let Some(first) = layers.first() {
            for object in &mut objects {
                if object.layer.is_none() {
                    object.layer = Some(first.name.clone());
                }
            }
        }
        if !layers.iter().any(|existing| existing.name == layer.name) {
            layers.push(layer.clone());
        }
        let mut skipped = 0_usize;
        for mut object in copied {
            object.layer = Some(layer.name.clone());
            if note_taken(&objects, &object) {
                skipped = skipped.saturating_add(1);
                continue;
            }
            let at = objects.partition_point(|other| other.tick <= object.tick);
            objects.insert(at, object);
        }
        if layers == target.layers && objects == target.objects {
            return Ok(skipped);
        }
        let edit = Edit(vec![
            Op::Layers {
                chart,
                old: target.layers.clone(),
                new: layers,
            },
            Op::Objects {
                chart,
                old: target.objects.clone(),
                new: objects,
            },
        ]);
        self.document = self.edited(chart, &edit)?;
        self.record(edit);
        Ok(skipped)
    }

    /// Layer name where new notes go, refusing hidden, ghosted or locked rows.
    pub(super) fn editable_layer(&self, index: usize) -> Result<Option<&LayerName>, Refusal> {
        let chart = self
            .document
            .document()
            .charts
            .get(self.chart)
            .ok_or(Refusal::NoChart)?;
        if chart.layers.is_empty() {
            return Ok(None);
        }
        let layer = chart
            .layers
            .get(index)
            .ok_or_else(|| Refusal::LayerName("no such layer".into()))?;
        match layer.view {
            LayerView::Hidden => Err(Refusal::LayerHidden(layer.name.to_string())),
            LayerView::Ghosted => Err(Refusal::LayerGhosted(layer.name.to_string())),
            LayerView::Shown if layer.locked => Err(Refusal::LayerLocked(layer.name.to_string())),
            LayerView::Shown => Ok(Some(&layer.name)),
        }
    }

    /// Whether an object is on a shown, unlocked layer.
    pub(super) fn is_editable(&self, object: &Object) -> bool {
        self.document
            .document()
            .charts
            .get(self.chart)
            .is_some_and(|chart| {
                chart
                    .layer_index(object)
                    .is_none_or(|index| chart.layers.get(index).is_some_and(Layer::is_editable))
            })
    }

    /// Human-readable name of an object's row, even when it has no explicit name.
    pub(super) fn object_layer_name(&self, object: &Object) -> String {
        self.document
            .document()
            .charts
            .get(self.chart)
            .and_then(|chart| chart.layers.get(chart.layer_index(object)?))
            .map_or_else(|| "Notes".into(), |layer| layer.name.to_string())
    }

    /// Compare effective layers, so an unnamed note on the first row counts as that row.
    pub(super) fn same_layer(&self, left: &Object, right: &Object) -> bool {
        self.document
            .document()
            .charts
            .get(self.chart)
            .is_some_and(|chart| chart.layer_index(left) == chart.layer_index(right))
    }

    /// Apply a layer change and any necessary note remapping as one validated edit.
    fn edit_layers(
        &mut self,
        change: impl FnOnce(&mut Vec<Layer>, &mut Vec<Object>) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        if self.plugin().is_none() {
            return Err(Refusal::ReadOnly);
        }
        let chart = self
            .document
            .document()
            .charts
            .get(self.chart)
            .ok_or(Refusal::NoChart)?;
        let mut layers = chart.layers.clone();
        let mut objects = chart.objects.clone();
        if layers.is_empty() {
            let notes =
                LayerName::new("Notes").map_err(|error| Refusal::LayerName(error.to_string()))?;
            layers.push(Layer {
                name: notes.clone(),
                colour: LayerColour::Blue,
                view: LayerView::Shown,
                locked: false,
            });
            for object in &mut objects {
                object.layer = Some(notes.clone());
            }
        } else {
            for object in &mut objects {
                if object.layer.is_none() {
                    object.layer = layers.first().map(|layer| layer.name.clone());
                }
            }
        }
        change(&mut layers, &mut objects)?;
        let mut ops = Vec::new();
        if layers != chart.layers {
            ops.push(Op::Layers {
                chart: self.chart,
                old: chart.layers.clone(),
                new: layers,
            });
        }
        if objects != chart.objects {
            ops.push(Op::Objects {
                chart: self.chart,
                old: chart.objects.clone(),
                new: objects,
            });
        }
        if ops.is_empty() {
            return Ok(());
        }
        let edit = Edit(ops);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::tests::*;
    use charter_core::{LaneOffset, TickOffset};
    use serde_json::{Value, json};

    #[test]
    fn opening_legacy_charts_migrates_each_without_editing_history() {
        let mut source =
            serde_json::to_value(document(json!([object(0, 0, "tap")])).document()).unwrap();
        for chart in source["charts"].as_array_mut().unwrap() {
            chart.as_object_mut().unwrap().remove("layers");
            if let Some(notes) = chart.get_mut("objects").and_then(Value::as_array_mut) {
                for note in notes {
                    note.as_object_mut().unwrap().remove("layer");
                }
            }
        }
        let mut song = Song::open(&serde_json::to_vec(&source).unwrap(), EDITABLE).unwrap();
        for chart in &song.document().document().charts {
            assert_eq!(chart.layers.len(), 1);
            assert_eq!(chart.layers[0].name.as_str(), "Layer 0");
            assert_eq!(chart.layers[0].colour, LayerColour::Blue);
            assert_eq!(chart.layers[0].view, LayerView::Shown);
            assert!(!chart.layers[0].locked);
            assert!(chart.objects.iter().all(|note| {
                note.layer
                    .as_ref()
                    .is_some_and(|layer| layer.as_str() == "Layer 0")
            }));
        }
        assert!(!song.is_modified());
        assert!(!song.can_undo());
        assert!(!song.can_redo());
        let saved: Value = serde_json::from_slice(&song.document().to_json().unwrap()).unwrap();
        assert_eq!(saved["charts"][0]["layers"][0]["name"], "Layer 0");
        assert_eq!(saved["charts"][0]["objects"][0]["layer"], "Layer 0");
        song.add_layer().unwrap();
        assert_eq!(song.layers()[0].name, "Layer 1");
        song.rename_layer(layer_index(0), "Other").unwrap();
        song.add_layer().unwrap();
        assert_eq!(song.layers()[0].name, "Layer 1");
    }

    #[test]
    fn constructing_a_song_migrates_legacy_charts_without_affecting_existing_layers() {
        let mut value =
            serde_json::to_value(document(json!([object(0, 0, "tap")])).document()).unwrap();
        value["charts"][0].as_object_mut().unwrap().remove("layers");
        value["charts"][0]["objects"][0]
            .as_object_mut()
            .unwrap()
            .remove("layer");
        value["charts"][1]["layers"] = json!([{"name": "Existing", "colour": "red"}]);
        value["charts"][1]["objects"] =
            json!([{"tick":240,"position":{"lane":{"index":1}},"kind":"tap","shape":"instant"}]);
        let valid = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let song = Song::new(valid, EDITABLE);
        assert_eq!(
            song.document().document().charts[0].layers[0].name.as_str(),
            "Layer 0"
        );
        assert_eq!(
            song.document().document().charts[1].layers[0].name.as_str(),
            "Existing"
        );
        assert!(
            song.document().document().charts[1].objects[0]
                .layer
                .is_none()
        );
        assert!(!song.is_modified());
    }

    #[test]
    fn layer_structure_is_one_undoable_edit_and_notes_keep_their_names_on_reorder() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        let original = song.document().document().clone();
        assert_eq!(song.layers()[0].name, "Layer 0");
        song.add_layer().unwrap();
        assert_eq!(song.layers().len(), 2);
        assert_eq!(song.active_layer(), layer_index(0));
        assert_eq!(
            song.document().document().charts[0].objects[0]
                .layer
                .as_ref()
                .unwrap()
                .as_str(),
            "Layer 0"
        );
        song.move_layer(layer_index(1), layer_index(0)).unwrap();
        assert_eq!(song.layers()[0].name, "Layer 0");
        song.undo().unwrap();
        song.undo().unwrap();
        assert_eq!(*song.document().document(), original);
        song.redo().unwrap();
        song.redo().unwrap();
        assert_eq!(song.layers()[0].name, "Layer 0");
    }

    #[test]
    fn layer_visibility_and_lock_protect_selection_and_placement() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.add_layer().unwrap();
        song.set_layer_view(layer_index(1), LayerView::Hidden)
            .unwrap();
        song.select_all();
        assert!(song.selection().next().is_none());
        song.set_active_layer(layer_index(1));
        assert!(matches!(
            song.toggle(at(0), lane(1), Kind::Tap),
            Err(Refusal::LayerHidden(_))
        ));
        song.set_layer_view(layer_index(1), LayerView::Ghosted)
            .unwrap();
        assert!(matches!(
            song.toggle(at(0), lane(1), Kind::Tap),
            Err(Refusal::LayerGhosted(_))
        ));
        song.set_layer_view(layer_index(1), LayerView::Shown)
            .unwrap();
        song.set_layer_locked(layer_index(1), true).unwrap();
        assert!(matches!(
            song.toggle(at(0), lane(1), Kind::Tap),
            Err(Refusal::LayerLocked(_))
        ));
        song.set_active_layer(layer_index(0));
        assert!(matches!(
            song.toggle(at(0), lane(0), Kind::Tap),
            Err(Refusal::OnLayer(_))
        ));
    }

    #[test]
    fn layer_copy_skips_occupied_spots_and_undo_restores_target() {
        let mut song = Song::new(
            document(json!([object(0, 0, "tap"), object(240, 1, "tap")])),
            EDITABLE,
        );
        song.add_layer().unwrap();
        let before = song.document().document().clone();
        song.open_chart(chart_index(1));
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.open_chart(chart_index(0));
        assert_eq!(
            song.copy_layer_to_chart(layer_index(1), chart_index(1))
                .unwrap(),
            1
        );
        assert_eq!(song.document().document().charts[1].objects.len(), 2);
        song.undo().unwrap();
        assert_eq!(song.document().document().charts[0], before.charts[0]);
        assert_eq!(song.document().document().charts[1].objects.len(), 1);
    }

    #[test]
    fn undoing_a_layer_copy_keeps_the_open_charts_solo() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.add_layer().unwrap();
        song.solo_layer(layer_index(0)).unwrap();
        song.copy_layer_to_chart(layer_index(1), chart_index(1))
            .unwrap();
        song.undo().unwrap();
        song.solo_layer(layer_index(0)).unwrap();
        let views: Vec<_> = song.layers().iter().map(|layer| layer.view).collect();
        assert_eq!(views, [LayerView::Shown, LayerView::Shown]);
    }

    #[test]
    fn deleting_a_layer_above_the_active_last_one_keeps_it_active() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.add_layer().unwrap();
        song.add_layer().unwrap();
        song.set_active_layer(layer_index(2));
        song.delete_layer(layer_index(0), LayerNotes::Delete)
            .unwrap();
        assert_eq!(song.layers()[song.active_layer().get()].name, "Layer 0");
    }

    #[test]
    fn undo_and_redo_keep_the_active_row_on_its_layer() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.add_layer().unwrap();
        song.add_layer().unwrap();
        song.set_active_layer(layer_index(2));
        song.delete_layer(layer_index(0), LayerNotes::Delete)
            .unwrap();
        song.undo().unwrap();
        assert_eq!(song.layers()[song.active_layer().get()].name, "Layer 0");
        song.redo().unwrap();
        assert_eq!(song.layers()[song.active_layer().get()].name, "Layer 0");
    }

    #[test]
    fn copying_into_a_layered_chart_explicitly_names_its_existing_implicit_notes() {
        let mut value =
            serde_json::to_value(document(json!([object(240, 1, "tap")])).document()).unwrap();
        value["charts"][1]["layers"] = json!([{"name":"Other","colour":"green"}]);
        value["charts"][1]["objects"] = json!([object(0, 0, "tap")]);
        value["charts"][1]["objects"][0]
            .as_object_mut()
            .unwrap()
            .remove("layer");
        let valid = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let mut song = Song::new(valid, EDITABLE);
        song.copy_layer_to_chart(layer_index(0), chart_index(1))
            .unwrap();
        assert_eq!(
            song.document().document().charts[1].objects[0]
                .layer
                .as_ref()
                .unwrap()
                .as_str(),
            "Other"
        );
    }

    #[test]
    fn layer_properties_undo_and_redo_exactly() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.add_layer().unwrap();
        for edit in [
            |editing: &mut Song| editing.rename_layer(layer_index(0), "Kick"),
            |editing: &mut Song| editing.recolour_layer(layer_index(0), LayerColour::Pink),
            |editing: &mut Song| editing.set_layer_view(layer_index(0), LayerView::Ghosted),
            |editing: &mut Song| editing.set_layer_locked(layer_index(0), true),
            |editing: &mut Song| editing.move_layer(layer_index(0), layer_index(1)),
            |editing: &mut Song| editing.solo_layer(layer_index(1)),
        ] {
            let before = song.document().document().clone();
            edit(&mut song).unwrap();
            let after = song.document().document().clone();
            assert_ne!(before, after);
            song.undo().unwrap();
            assert_eq!(*song.document().document(), before);
            song.redo().unwrap();
            assert_eq!(*song.document().document(), after);
        }
    }

    #[test]
    fn solo_restores_hidden_and_ghosted_views_and_manual_view_change_forgets_it() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.add_layer().unwrap();
        song.set_layer_view(layer_index(1), LayerView::Hidden)
            .unwrap();
        song.solo_layer(layer_index(1)).unwrap();
        assert_eq!(song.layers()[1].view, LayerView::Shown);
        assert_eq!(song.layers()[0].view, LayerView::Ghosted);
        song.solo_layer(layer_index(1)).unwrap();
        assert_eq!(song.layers()[1].view, LayerView::Hidden);
        assert_eq!(song.layers()[0].view, LayerView::Shown);
        song.solo_layer(layer_index(1)).unwrap();
        song.set_layer_view(layer_index(0), LayerView::Hidden)
            .unwrap();
        song.solo_layer(layer_index(1)).unwrap();
        assert_eq!(song.layers()[0].view, LayerView::Hidden);
    }

    #[test]
    fn undoing_a_note_edit_does_not_forget_the_solo_views() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.add_layer().unwrap();
        song.solo_layer(layer_index(1)).unwrap();
        song.set_active_layer(layer_index(1));
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.undo().unwrap();
        song.solo_layer(layer_index(1)).unwrap();
        assert_eq!(song.layers()[1].view, LayerView::Shown);
        assert_eq!(song.layers()[0].view, LayerView::Shown);
    }

    #[test]
    fn layer_names_colours_and_layer_look_follow_the_active_chart() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        assert_eq!(
            song.layer_look(&song.chart().unwrap().objects[0]),
            (LayerView::Shown, None)
        );
        song.add_layer().unwrap();
        assert_eq!(song.layers()[0].name, "Layer 1");
        assert_eq!(song.layers()[0].colour, LayerColour::Red);
        assert_eq!(
            song.layer_look(&song.chart().unwrap().objects[0]),
            (LayerView::Shown, Some(LayerColour::Blue))
        );
        song.add_layer().unwrap();
        assert_eq!(song.layers()[0].name, "Layer 2");
        assert_eq!(song.layers()[0].colour, LayerColour::Orange);
        song.set_active_layer(layer_index(999));
        assert_eq!(song.active_layer(), layer_index(2));
        assert!(matches!(
            song.rename_layer(layer_index(0), "Layer 0"),
            Err(Refusal::LayerName(_))
        ));
        assert!(matches!(
            song.rename_layer(layer_index(0), " bad "),
            Err(Refusal::LayerName(_))
        ));
        assert!(matches!(
            song.rename_layer(layer_index(44), "ok"),
            Err(Refusal::LayerName(_))
        ));
        song.rename_layer(layer_index(0), "Kick").unwrap();
        song.set_layer_view(layer_index(2), LayerView::Hidden)
            .unwrap();
        assert_eq!(
            song.layer_look(&song.chart().unwrap().objects[0]),
            (LayerView::Hidden, Some(LayerColour::Blue))
        );
    }

    #[test]
    fn protected_notes_cannot_be_selected_by_any_selection_path() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.add_layer().unwrap();
        for view in [LayerView::Ghosted, LayerView::Hidden] {
            song.set_layer_view(layer_index(1), view).unwrap();
            song.select([note_at(0, 0)], Selecting::Replace);
            assert_eq!(song.selection().count(), 0);
            song.select_between(at(0)..=at(1), lane(0)..=lane(3), Selecting::Replace);
            assert_eq!(song.selection().count(), 0);
            song.select_layer(layer_index(1));
            assert_eq!(song.selection().count(), 0);
            song.select_all();
            assert_eq!(song.selection().count(), 0);
        }
        song.set_layer_view(layer_index(1), LayerView::Shown)
            .unwrap();
        song.set_layer_locked(layer_index(1), true).unwrap();
        song.select_all();
        assert_eq!(song.selection().count(), 0);
        song.set_layer_locked(layer_index(1), false).unwrap();
        song.select_layer(layer_index(1));
        assert_eq!(song.selection().count(), 1);
        song.select_layer(layer_index(99));
        assert_eq!(song.selection().count(), 0);
    }

    #[test]
    fn a_read_only_song_refuses_layer_edits() {
        let mut song = Song::new(document(json!([])), &[]);
        assert!(matches!(song.add_layer(), Err(Refusal::ReadOnly)));
        assert!(matches!(
            song.copy_layer_to_chart(layer_index(0), chart_index(1)),
            Err(Refusal::ReadOnly)
        ));
        assert!(matches!(
            song.set_layer_view(layer_index(0), LayerView::Hidden),
            Err(Refusal::ReadOnly)
        ));
        assert!(matches!(
            song.rename_layer(layer_index(0), "Kick"),
            Err(Refusal::ReadOnly)
        ));
    }

    #[test]
    fn a_layer_edit_refused_by_the_plugin_keeps_the_document_and_history_unchanged() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), &[&NoExtraLayers]);
        let before = song.document().document().clone();
        assert!(matches!(song.add_layer(), Err(Refusal::Game(_))));
        assert_eq!(*song.document().document(), before);
        assert!(!song.is_modified());
    }

    #[test]
    fn layer_no_ops_do_not_materialise_an_implicit_chart_or_add_history() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        let before = song.document().document().clone();
        song.move_selection_to_layer(layer_index(0)).unwrap();
        song.select_all();
        song.move_selection_to_layer(layer_index(0)).unwrap();
        song.rename_layer(layer_index(0), "Layer 0").unwrap();
        song.recolour_layer(layer_index(0), LayerColour::Blue)
            .unwrap();
        song.set_layer_view(layer_index(0), LayerView::Shown)
            .unwrap();
        song.set_layer_locked(layer_index(0), false).unwrap();
        song.move_layer(layer_index(0), layer_index(0)).unwrap();
        assert_eq!(*song.document().document(), before);
        assert!(!song.is_modified());
    }

    #[test]
    fn copying_an_empty_existing_layer_does_not_create_an_undo_step() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.add_layer().unwrap();
        song.mark_saved();
        assert_eq!(
            song.copy_layer_to_chart(layer_index(0), chart_index(0))
                .unwrap(),
            0
        );
        assert!(!song.is_modified());
    }

    #[test]
    fn moving_merging_deleting_and_copying_layers_each_undo_and_redo_the_exact_document() {
        let initial = document(json!([object(0, 0, "tap"), object(240, 1, "tap")]));
        let mut song = Song::new(initial, EDITABLE);
        song.add_layer().unwrap();
        song.select([note_at(0, 0)], Selecting::Replace);
        let before_move = song.document().document().clone();
        song.move_selection_to_layer(layer_index(0)).unwrap();
        let after_move = song.document().document().clone();
        song.undo().unwrap();
        assert_eq!(*song.document().document(), before_move);
        song.redo().unwrap();
        assert_eq!(*song.document().document(), after_move);

        let before_copy = song.document().document().clone();
        song.copy_layer_to_chart(layer_index(0), chart_index(1))
            .unwrap();
        let after_copy = song.document().document().clone();
        song.undo().unwrap();
        assert_eq!(*song.document().document(), before_copy);
        song.redo().unwrap();
        assert_eq!(*song.document().document(), after_copy);

        let before_merge = song.document().document().clone();
        song.merge_layer_down(layer_index(0)).unwrap();
        let after_merge = song.document().document().clone();
        song.undo().unwrap();
        assert_eq!(*song.document().document(), before_merge);
        song.redo().unwrap();
        assert_eq!(*song.document().document(), after_merge);

        song.undo().unwrap();
        let before_delete = song.document().document().clone();
        song.delete_layer(layer_index(0), LayerNotes::Delete)
            .unwrap();
        let after_delete = song.document().document().clone();
        song.undo().unwrap();
        assert_eq!(*song.document().document(), before_delete);
        song.redo().unwrap();
        assert_eq!(*song.document().document(), after_delete);
    }

    #[test]
    fn layer_notes_move_merge_delete_and_reject_last_row() {
        let mut song = Song::new(
            document(json!([object(0, 0, "tap"), object(240, 1, "tap")])),
            EDITABLE,
        );
        song.add_layer().unwrap();
        song.select_layer(layer_index(1));
        assert_eq!(song.selection().count(), 2);
        song.move_selection_to_layer(layer_index(0)).unwrap();
        assert_eq!(song.layers()[0].notes, 2);
        assert_eq!(song.layers()[1].notes, 0);
        song.undo().unwrap();
        assert_eq!(song.layers()[1].notes, 2);
        song.move_layer(layer_index(1), layer_index(0)).unwrap();
        song.merge_layer_down(layer_index(0)).unwrap();
        assert_eq!(song.layers().len(), 1);
        assert_eq!(song.layers()[0].notes, 2);
        assert!(matches!(
            song.delete_layer(layer_index(0), LayerNotes::Delete),
            Err(Refusal::LastLayer)
        ));
        song.undo().unwrap();
        song.delete_layer(layer_index(0), LayerNotes::Delete)
            .unwrap();
        assert!(song.document().document().charts[0].objects.is_empty());
    }

    #[test]
    fn switching_charts_keeps_their_active_rows_and_pasting_uses_the_active_name() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.add_layer().unwrap();
        song.set_active_layer(layer_index(1));
        song.open_chart(chart_index(1));
        assert_eq!(song.active_layer(), layer_index(0));
        song.add_layer().unwrap();
        song.open_chart(chart_index(0));
        assert_eq!(song.active_layer(), layer_index(1));
        let mut clip: Object = serde_json::from_value(object(0, 2, "tap")).unwrap();
        clip.layer = Some(LayerName::new("Layer 1").unwrap());
        song.paste(&[clip], at(240)).unwrap();
        assert_eq!(
            song.document().document().charts[0].objects[1]
                .layer
                .as_ref()
                .unwrap()
                .as_str(),
            "Layer 0"
        );
    }

    #[test]
    fn duplicating_selected_notes_keeps_each_source_layer_while_paste_uses_the_active_one() {
        let mut song = Song::new(
            document(json!([object(0, 0, "tap"), object(0, 1, "tap")])),
            EDITABLE,
        );
        song.add_layer().unwrap();
        song.select([note_at(0, 1)], Selecting::Replace);
        song.move_selection_to_layer(layer_index(0)).unwrap();
        song.select_all();
        assert_eq!(song.duplicate_selection(at(240)).unwrap(), 2);
        let chart = &song.document().document().charts[0];
        assert_eq!(chart.objects[2].layer, chart.objects[0].layer);
        assert_eq!(chart.objects[3].layer, chart.objects[1].layer);
        let clip = song.copied();
        song.set_active_layer(layer_index(1));
        song.paste(&clip, at(480)).unwrap();
        assert_eq!(
            song.document().document().charts[0].objects[4]
                .layer
                .as_ref()
                .unwrap()
                .as_str(),
            "Layer 0"
        );
    }

    #[test]
    fn duplication_uses_source_layers_even_when_the_active_layer_is_hidden() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.add_layer().unwrap();
        song.set_layer_view(layer_index(0), LayerView::Hidden)
            .unwrap();
        song.select_layer(layer_index(1));
        assert_eq!(song.duplicate_selection(at(240)).unwrap(), 1);
        assert_eq!(song.layers()[1].notes, 2);
    }

    #[test]
    fn placement_and_movement_into_another_layer_report_its_name_without_changing_notes() {
        let mut song = Song::new(
            document(json!([object(0, 0, "tap"), object(240, 1, "tap")])),
            EDITABLE,
        );
        song.add_layer().unwrap();
        let original = song.document().document().clone();
        assert!(
            matches!(song.toggle(at(0), lane(0), Kind::Tap), Err(Refusal::OnLayer(name)) if name == "Layer 0")
        );
        assert!(
            matches!(song.add_taps([(at(240), lane(1))]), Err(Refusal::OnLayer(name)) if name == "Layer 0")
        );
        song.select([note_at(0, 0)], Selecting::Replace);
        song.move_selection_to_layer(layer_index(0)).unwrap();
        assert!(
            matches!(song.move_selection(TickOffset::new(240), LaneOffset::new(1)), Err(Refusal::OnLayer(name)) if name == "Layer 0")
        );
        song.undo().unwrap();
        assert_eq!(*song.document().document(), original);
    }

    #[test]
    fn an_unnamed_note_on_a_real_first_layer_is_not_mistaken_for_another_layer() {
        let mut value =
            serde_json::to_value(document(json!([object(0, 0, "tap")])).document()).unwrap();
        value["charts"][0]["layers"] =
            json!([{"name":"Kick","colour":"red"}, {"name":"Snare","colour":"blue"}]);
        value["charts"][0]["objects"][0]
            .as_object_mut()
            .unwrap()
            .remove("layer");
        let valid = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let mut song = Song::new(valid, EDITABLE);
        assert_eq!(
            song.toggle(at(0), lane(0), Kind::Tap).unwrap(),
            Toggled::Removed
        );
    }
}
