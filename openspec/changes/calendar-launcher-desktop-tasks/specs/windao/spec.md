## ADDED Requirements

### Requirement: Calendar-first capsule navigation
The capsule SHALL open on the calendar when enabled and offer a menu beside the page title.

#### Scenario: Opening the menu
- **WHEN** the user expands the capsule and selects the title menu
- **THEN** available features are grouped under 效率, 内容 and 工具, and selecting an item opens its actual destination

### Requirement: Independent desktop task window
The application SHALL allow the user to create and complete tasks from a separate desktop window using the same workspace as the workbench.

#### Scenario: Using tasks after closing the workbench
- **WHEN** the user opens desktop tasks and closes the workbench or collapses the capsule
- **THEN** desktop tasks remain usable and changes appear when the workbench reopens

### Requirement: Explicit application exit
The application SHALL provide a visible quit command in the workbench and capsule menu.

#### Scenario: Quitting from a child window
- **WHEN** the user activates the workbench quit command
- **THEN** the application closes the root window and all of its child windows
