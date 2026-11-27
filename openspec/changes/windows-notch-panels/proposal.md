## Why

Windows 0.1.5 only renders six destinations inside the capsule. The rest of the launcher opens separate windows, so users cannot switch tools inside the notch as on macOS.

## What Changes

- Add the macOS notch destinations to the Windows capsule and page settings.
- Keep calendar as the first destination on expansion when enabled.
- Preserve existing page settings and provide an overflow entry when there is insufficient space.
- Provide inline calculator and news views and reuse existing Windows views for the remaining tools.

## Capabilities

### New Capabilities

- `windows-notch-panels` lets users switch tools within the Windows capsule.

## Impact

The change affects capsule routing, the Windows workbench view composition, and migration of saved capsule page preferences.
