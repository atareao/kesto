# Optional container stop before backup

## Scenario: No containers specified → backup proceeds normally

Given a backup target configured for a database
And `KESTO_STOP_CONTAINERS` is not set (no containers specified)
When the backup runs
Then no Docker containers are stopped or started
And the backup proceeds normally

## Scenario: One or more containers specified → containers are stopped, backup taken, restarted after

Given `KESTO_STOP_CONTAINERS` lists container names/IDs
And those containers are running
When the backup runs
Then each listed container is stopped before the backup starts
And the backup is taken against the database
And each stopped container is restarted after the backup completes

## Scenario: Backup fails → containers are still restarted and error logged

Given `KESTO_STOP_CONTAINERS` lists container names/IDs
And those containers are running
When the backup fails
Then each stopped container is restarted despite the backup failure
And the failure is logged
And the backup error is surfaced

## Scenario: A container cannot be stopped → already stopped containers are rolled back, the row is `failed` and the final event is published

Given `KESTO_STOP_CONTAINERS` lists `web,db`
And both containers are running
And stopping `db` fails
When the backup runs
Then no backup is attempted
And `web` is restarted before the error is returned
And `db` is never sent a stop request
And the execution row has `status: failed`, a populated `finished_at` and an `error_message` naming `db`
And an `execution_progress` event is published for that execution with `status: failed` and `current_action: "terminando"`

## Scenario: A container cannot be restarted after a successful backup → the result stays `success` and the operator is warned

Given `KESTO_STOP_CONTAINERS` lists `web,db`
And both containers were stopped successfully
And the backup completes successfully
And restarting `db` fails
When the execution is closed
Then the execution row has `status: success` and its `snapshot_id`
And `error_message` holds a non-fatal warning naming `db`
And an `execution_progress` event carries that warning in `current_action` with `status: success`
And the final `execution_progress` event is published after the warning with `status: success` and `current_action: "terminando"`
And the warning tells the operator to restart `db` manually
