---
type: Format
title: .ssc
description: MSD5 successor to .sm with per-chart timing and the widest gimmick vocabulary of the surveyed formats.
resource: https://raw.githubusercontent.com/stepmania/stepmania/5_1-new/Docs/SimfileFormats/ssc_msd5.txt
tags: [format, stepmania, timing]
status: draft
sources:
  - id: ssc-spec
    resource: https://raw.githubusercontent.com/stepmania/stepmania/5_1-new/Docs/SimfileFormats/ssc_msd5.txt
    title: ssc_msd5.txt
  - id: ssc-changelog
    resource: https://raw.githubusercontent.com/stepmania/stepmania/5_1-new/Docs/Changelog_SSCformat.txt
    title: SSC format changelog
---

# .ssc

MSD5 successor with repeated `#NOTEDATA` sections and **per-chart timing**.

Models `#OFFSET`, `#BPMS`, `#STOPS`, `#DELAYS`, `#TIMESIGNATURES`, `#TICKCOUNTS`, `#COMBOS`,
`#SPEEDS`, `#SCROLLS`, `#FAKES`, `#LABELS`, attacks, display BPM, and warps.

This is the most timing-expressive format in the surveyed set. Anything targeting it must
represent non-bijective timing: warps skip time, stops and delays pause it. A timestamp cannot be
mapped back to a row by simple inversion.

Which of these a chart editor must actually support is open — see
[gimmick coverage](../topics/gimmick-coverage.md).
