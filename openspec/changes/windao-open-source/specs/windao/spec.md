## ADDED Requirements

### Requirement: Independent Windows application

WinDao SHALL build from its own repository and store data in a dedicated per-user profile without an activation code.

#### Scenario: User opens an installed or portable copy

- **WHEN** the user runs WinDao on Windows 11 x64
- **THEN** menus, media controls and local data editing are immediately available

### Requirement: Desktop capsule placement

The capsule SHALL dock on any of the four display edges and SHALL allow explicit repositioning across monitors.

#### Scenario: User repositions the capsule

- **WHEN** the user Ctrl-drags the collapsed capsule to another display
- **THEN** WinDao saves the destination display, closest edge and along-edge position

### Requirement: Local information and reminders

The workbench SHALL show memory, processor and network usage, world clocks, current track lyrics and recent WinDao reminders.

#### Scenario: A reminder is accepted by Windows

- **WHEN** Windows accepts a scheduled WinDao reminder
- **THEN** WinDao persists it with the delivery record without repeating it on a storage retry

#### Scenario: A supported chat application flashes its taskbar button

- **WHEN** the user has enabled chat taskbar history and the flashing executable matches a supported name and verified publisher
- **THEN** WinDao records the application name and local observation time without accessing chat content

### Requirement: Daily desktop utilities

The workbench SHALL provide saved-date countdowns, on-demand gold quotes, a local earnings estimate, optional session clipboard history and keyboard page navigation. Calendar views SHALL show lunar dates and solar terms for the supported date range.

#### Scenario: User checks a selected date

- **WHEN** the user selects a day between January 1, 1901 and December 31, 2100
- **THEN** the calendar shows its corresponding lunar date and any solar term without requesting a network connection

#### Scenario: User moves between workbench pages

- **WHEN** the user presses Ctrl+Tab or Ctrl+Shift+Tab in the focused workbench with no modal open
- **THEN** the workspace selects the next or previous page and wraps at the ends of the list

### Requirement: On-demand data and local widgets

The workbench SHALL allow the user to look up city forecasts, inspect paired Bluetooth devices, and install, disable or remove bundled or imported declarative widgets.

#### Scenario: User imports a widget

- **WHEN** the user selects a valid local JSON manifest
- **THEN** WinDao renders its text, links or checklist and saves the widget state on the device

#### Scenario: A Bluetooth battery is unavailable

- **WHEN** Windows or the peripheral provides no standard battery value
- **THEN** the wireless page reports the actual missing value or failure state
