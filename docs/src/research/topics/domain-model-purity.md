---
type: Open Decision
title: Domain model purity
description: Whether editor state such as selection and highlight may live on domain objects.
tags: [topic, data-model, architecture, open]
status: draft
---

# Domain model purity

## Observed

- Editor on Fire mixes highlight and sort flags into note state.
- Moonscraper goes the other way at the undo layer: its `CommandStack` stores camera and selection
  **snapshots alongside** edit commands, keeping view state adjacent to rather than inside the
  chart data.

## Open decision

- Domain objects carry no editor state; selection and highlight live in a parallel structure
- Domain objects carry transient editor state for convenience
- Editor state is a separate layer that references domain objects by identity

The third needs stable identity for notes, which interacts with
[row resolution](row-resolution.md) and with how edits are addressed in
[undo model](undo-model.md).

## Evidence

Earlier research stated "do not use the domain model to store editor UI state" as a rule. It is one
project's experience generalized into a principle, with no recorded failure from EOF's approach.

Grade: **inferred**.

## Draft position

Provisionally resolved for the format: no editor state in chart data. See [notes](../../format.md#objects).
