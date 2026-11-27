## Context

The Windows capsule renders its common views directly. The three-column launcher dispatches all other pages to the workbench.

## Decisions

The capsule hosts existing workbench page bodies without their surrounding window navigation. Keep shared state for each tool across surfaces. Keep the bottom switcher anchored below independently scrollable content. An overflow control opens the searchable launcher when tools exceed the available width. Append new page preferences on deserialization of a valid old page list. News follows the macOS visibility default and can be enabled in settings.
