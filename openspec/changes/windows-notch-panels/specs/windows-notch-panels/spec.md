## ADDED Requirements

### Requirement: Tools open inside the Windows capsule

The capsule SHALL render enabled Mac-equivalent destinations inside the capsule. Calendar SHALL open first when enabled.

#### Scenario: Select a tool from the capsule launcher

- **WHEN** the user selects an enabled notch tool in the three-column launcher
- **THEN** the tool SHALL render within the capsule

#### Scenario: Open a tool on a narrow display

- **WHEN** all enabled tools do not fit in the bottom switcher
- **THEN** the capsule SHALL offer a More control that exposes the enabled tools in the launcher

### Requirement: Prior capsule settings survive upgrade

The app SHALL retain prior capsule page order and visibility and append new destinations.

#### Scenario: Read settings written by Windows 0.1.5

- **WHEN** the app reads a valid original six-page preference list
- **THEN** it SHALL preserve those entries and add the newly available pages
