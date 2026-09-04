# hidden-groups Specification

## ADDED Requirements

### Requirement: Unscoped search skips hidden groups
The system SHALL read `ECPHORY_HIDDEN_GROUPS` (comma-separated group ids, whitespace trimmed, empty entries ignored) at open, and a search whose options carry no `group_id` SHALL exclude every episode whose `group_id` is in that set. The exclusion happens in the post-filter pass shared by MCP, REST and CLI, so all three entry points agree.

#### Scenario: Unscoped search
- **WHEN** `ECPHORY_HIDDEN_GROUPS=todos` and a search runs with no `group_id`
- **THEN** no episode in group `todos` appears in the results, regardless of score

#### Scenario: Variable unset
- **WHEN** `ECPHORY_HIDDEN_GROUPS` is unset or empty
- **THEN** search behaves exactly as before this change

### Requirement: Naming a group opts in
A search that sets `group_id` SHALL apply only the existing group filter; a hidden group named explicitly SHALL be returned normally.

#### Scenario: Scoped to a hidden group
- **WHEN** `ECPHORY_HIDDEN_GROUPS=todos` and a search runs with `group_id=todos`
- **THEN** episodes in `todos` are returned as they were before this change

### Requirement: Hidden configuration is observable
`GET /api/v1/status` SHALL include `hidden_groups` as a JSON array of the configured group ids (empty when none), so an operator can confirm the deployment without reading its environment.

#### Scenario: Status check
- **WHEN** the service runs with `ECPHORY_HIDDEN_GROUPS=todos`
- **THEN** `/api/v1/status` contains `"hidden_groups": ["todos"]`
