## ADDED Requirements

### Requirement: NotchCalendar product icon
The application SHALL use its calendar artwork for executable resources, installer, shell shortcuts, tray, windows and MSIX tiles.

#### Scenario: Inspect either download
- **WHEN** the portable executable or installer is packaged
- **THEN** its icon group contains the calendar artwork at all nine supported sizes

### Requirement: Desktop tool navigation
The expanded calendar SHALL offer the fourteen macOS menu destinations in their efficiency, content and tools groups alongside existing Windows features.

#### Scenario: Use a tool
- **WHEN** the user selects a tool in the calendar title menu
- **THEN** its working Windows view or the corresponding Windows system tool opens

#### Scenario: Leave a verification session
- **WHEN** the user leaves the authenticator or closes the workbench
- **THEN** its decrypted accounts are cleared and the next visit requires unlocking
