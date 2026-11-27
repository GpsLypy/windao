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

### Requirement: Expanded panels prioritize content

The capsule SHALL identify the active tool in its header and use compact layouts for inline tool content. Setup details and secondary actions SHALL remain available through the relevant control or layout section.

#### Scenario: Open the file shelf

- **WHEN** the user opens Files in the capsule
- **THEN** the capsule SHALL show the add controls followed by file rows or a short drop hint without repeating the Files heading
- **AND** successful operations SHALL show brief feedback while failures remain visible

#### Scenario: Open a tool without results

- **WHEN** an inline tool has no results yet
- **THEN** the tool SHALL expose its primary action without allocating space for empty output or setup details unrelated to that action

### Requirement: Capsule transitions preserve the dock position

The collapsed capsule and the expanded panel SHALL share a backing window at the selected dock position during an open or close. The visible silhouette SHALL limit the window region. Panel content SHALL crossfade with the residents and the transition SHALL preserve its current position and velocity when interrupted. The app SHALL skip the animation when reduced motion is enabled.

#### Scenario: Move from the capsule to the expanding panel

- **WHEN** the pointer dwells over the compact clock and moves into the opening panel
- **THEN** the panel SHALL stay open while the pointer is inside or near its edge
- **AND** a resident notification control SHALL retain its own click target

#### Scenario: Close the panel while the pointer is over the clock

- **WHEN** the user explicitly closes the panel with the pointer on the compact hover target
- **THEN** hover SHALL remain suppressed until the pointer leaves that target
