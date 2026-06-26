# conductor

A small command-line front-end for the Linux Ops Suite's state: take and
restore snapshots, and inspect findings. Each subcommand maps to one tool group.

## Usage

```
conductor workstate snapshot     write a fresh snapshot
conductor workstate refresh      re-stamp it as current
conductor workstate status       show freshness and counts

conductor rewind capture         save the snapshot as a restore point
conductor rewind list            list restore points
conductor rewind restore <id>    restore a saved point

conductor bulwark show <id>      show one finding
conductor bulwark check          summarise findings
conductor bulwark tripwire       report high-severity drift
```

Global flags: `--json`, `--no-color`, `--data-dir <DIR>`.

State lives under `$XDG_DATA_HOME/linux-ops-suite` by default.
