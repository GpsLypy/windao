## Why

The Windows workbench, settings and capsule use different button shapes, icon colors and selection markers. Apply Fluent control patterns across these surfaces so users can recognize the same interactions wherever they appear.

## What Changes

- Give workbench and settings navigation a shared selection indicator, focus outline and neutral surfaces.
- Use switches for immediately applied binary settings and dropdowns for appearance choices.
- Apply the shared navigation treatment to the capsule launcher using the active capsule skin.

## Capabilities

### New Capabilities

- `windows-fluent-ui` defines Windows navigation and settings presentation.

### Modified Capabilities

None.

## Impact

Changes the Rust egui presentation for the Windows app. Workspace schema, feature availability and capsule entry points keep their existing behavior.
