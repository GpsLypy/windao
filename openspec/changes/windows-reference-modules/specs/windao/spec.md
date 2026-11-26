## ADDED Requirements

### Requirement: Account-scoped AI usage
The application SHALL distinguish local token counts from subscription percentages, credits and currency balances and retain the last successful reading per account.

#### Scenario: Pinning an account
- **WHEN** a user selects an account and enables its capsule display
- **THEN** the capsule shows that account's reading and refreshes its selected source every five minutes

#### Scenario: Third-party access expires
- **WHEN** a service rejects a credential or a user-selected file becomes unavailable
- **THEN** the account page preserves the previous reading and its observation time and reports the new failure

### Requirement: Mail and calendar
The application SHALL support mailbox reading, user-controlled message actions, invitation import and read-only ICS, CalDAV and Exchange ActiveSync sources.

#### Scenario: An invitation and a subscription include the same meeting
- **WHEN** they share an event identity and recurrence instance
- **THEN** they appear once and generate at most one reminder for the instance, while each source retains its own cache

### Requirement: Configurable Windows capsule
The application SHALL expose calendar-first navigation, configurable resident items, themes, media controls and system notifications that reflect available platform capabilities.

#### Scenario: A player omits timeline information
- **WHEN** the capsule receives a song title without a usable timeline
- **THEN** it shows the title and only offers supported playback actions

### Requirement: Local tools and distribution
The application SHALL offer persistent local task and clipboard tools, a file shelf, health reminders, weather, installable widgets and verified updates.

#### Scenario: An installer differs from the selected release
- **WHEN** its size or SHA-256 differs from the published metadata
- **THEN** the application reports the failed verification and leaves the current version running
