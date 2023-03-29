# Changelog

## Unreleased

## v0.2.0 - 2023-03-29
- Add `--idle-timeout` flag to shut down after a specified time with no connections
- Add `--from-systemd` flag for systemd socket activation support (linux only, enabled w/ `systemd` feature)
- Support systemd journal logging with [`systemd_journal_logger`](https://docs.rs/systemd-journal-logger)
- Allow custom injection of filename, line, and column with `%f`, `%l`, and `%c` in editor command
- Automatically pass cursor line/column to recognized editors
- Switch from `inotify` to `notify` for file watching, **should now compile on Mac/Windows, but untested**
- Update dependencies
- Reduce extra writes to files with debouncing and hashing, configurable with the `--delay` flag
- Add example systemd units for socket activation

## v0.1.0 - 2021-02-05

Initial release
- works
- linux only (inotify required)
