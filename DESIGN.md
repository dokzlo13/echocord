# Signal Bridge

Mode: Operate. This is the accepted production implementation direction for the Windows desktop control surface. Broader aesthetic refinement is deferred until real use; future multiple-source/destination studies are not current controls.

## Visual contract

Charcoal blue base `#242e36`, raised controls `#2c3942`, inset signal tracks `#172128`, primary text `#edf2f1`, secondary text `#b4c2c9`, seafoam `#9bd9c7`. Native Segoe UI, compact fixed type scale, tabular measurements. Seafoam identifies selection, signal and meaningful actions; warm colors identify actionable failures. No remote fonts or decorative charts.

Three clear regions: recording input, shared mix, Discord destination. Spatial grouping replaces graph boxes/connectors. Input and mix meters have 8px cores with a dBFS scale; adjustable master volume is a separate horizontal Mantine slider with a tactile thumb and explicit accessible label. Capture queue duration/occupancy belongs beside the input, with its occupied/total frame fraction and measurement boundary stated. The persistent transport owns mute and connect/cancel/stop.

The entire main surface, including pending/error actions, fits 480×600 and 420×540 CSS pixels without document or internal scrolling. Drawers contain searchable selection lists and secondary details; those lists may scroll. Long route names truncate on the main surface and remain readable in drawers. When error and pending bands coexist, secondary source detail is moved to the picker and meter tick density compresses; signal readings, confirmed route and all recovery actions remain visible. Error summaries lead to complete details without hiding essential recovery actions.

## Component and contract plan

- App subscribes to stable control state only. It owns a staged Selection draft, explicit apply/discard, authoritative confirmed route identity and persistent transport.
- Source drawer searches available recording endpoints; target drawer groups searchable voice rooms by server/category and uses actual catalog icons with an initials fallback. Picking closes the drawer and stages a draft; it does not dispatch selection or connect.
- Bot drawer has one Test & Save form using configureBot, an ephemeral password field, clear busy/success/failure state and first-run guidance. Mute/stop stay accessible during drawer operations.
- Meter and diagnostics components alone subscribe to adapter telemetry `{sessionId,sourceId,capture,visible}`. Ref/DOM updates are capped near 20Hz, stop while document/native visibility is false, and read the latest observation on resume. The input meter uses capture.levelDbfs; the mix meter uses actual nullable capture.outputLevelDbfs. No gain arithmetic or delivery claim.
- Apply uses the existing applyRoute API. Partial selection confirmation remains visible; failed/cancelled operations reconcile the remaining draft against snapshots and never claim atomic rollback. Cleanup failure blocks further apply/connect while allowing retry disconnect.

## Review and validation seam

Owner feedback in the current epoch identifies the near-full-width selection drawers as oversized for their purpose. Their containment and size are under bounded EC-021 review; narrower panels, anchored pickers and separate windows are proposals pending research and rendered review. Keep essential transport accessible while resolving this correction. The remaining visual contract stays in force.

Owner accepted the private refined Signal Bridge direction before implementation. This document is the implementation authority. Production screenshots, behavior, keyboard, fit, rendering and visibility evidence are collected privately; sanitized results and remaining native acceptance are recorded in EC-013. Coordinator owns independent review, native shell integration and final acceptance.
