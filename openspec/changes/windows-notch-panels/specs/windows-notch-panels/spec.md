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

### Requirement: AI chat and translation use embedded web pages

The Windows capsule SHALL embed the official DeepSeek chat page and offer web translation through Kingsoft, Baidu, and Alibaba. The embedded page SHALL follow the size and visibility of its capsule content area.

#### Scenario: Switch away from an embedded web page

- **WHEN** the user opens the launcher, switches tools, or collapses the capsule
- **THEN** the web page SHALL hide and its session SHALL remain available when the user returns

#### Scenario: The web runtime or page cannot load

- **WHEN** the WebView2 Runtime is unavailable or navigation fails
- **THEN** the capsule SHALL display a recovery action and allow the user to open the official site in their browser

### Requirement: Focus and monitoring panels use aligned layouts

The capsule SHALL center the timer, presets, and controls on a shared axis and align the CPU and memory headings, values, and charts.

#### Scenario: A CPU sample is initially unavailable

- **WHEN** the user opens the monitor before a CPU sample is available
- **THEN** the missing value SHALL occupy the same space as a numeric value

### Requirement: Expanded panels use the macOS visual hierarchy

The Windows capsule SHALL give its inline tools a shared visual hierarchy for controls, content cards, headings, and secondary text. The shell SHALL reserve space for the dock independently of the contents of each tool.

#### Scenario: Resize or change the order of an expanded capsule

- **WHEN** the dock no longer fits every enabled tool
- **THEN** the dock SHALL retain the calendar and current tool when both are enabled and space permits, and expose the remaining tools through More

#### Scenario: Switch capture modes in a narrow capsule

- **WHEN** the user switches between screenshot and recording at the minimum panel size
- **THEN** the capsule SHALL show similarly sized actions for the selected mode while keeping the recording folder legible above the dock
- **AND** Windows versions without region recording SHALL show the application recording action
