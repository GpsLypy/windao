## ADDED Requirements

### Requirement: Visual states follow a common pattern

The Windows workbench, settings navigation and capsule launcher SHALL use consistent selection and focus indicators. Controls SHALL use the active theme or capsule skin.

#### Scenario: Navigate with the keyboard

- **WHEN** a user tabs to a navigation item or activates it
- **THEN** the interface shows focus separately from the current selection

### Requirement: Settings controls match the type of choice

The Windows settings page SHALL use named switches for immediately applied binary preferences and labeled dropdowns for exclusive appearance choices.

#### Scenario: Update appearance

- **WHEN** the user selects a different capsule skin or shape
- **THEN** the app applies and persists the selected option through the existing settings path
