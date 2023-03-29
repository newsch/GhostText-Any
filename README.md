# GhostText-Any

A [GhostText](https://github.com/GhostText/GhostText) server for any `$EDITOR`.

_Built on idanarye's [`ghost-text-file`](https://github.com/idanarye/ghost-text-file)._

GhostText-Any allows you to edit any text box in your browser (Firefox/Chromium-based) with anything you can set your `$EDITOR` to.
It does this by saving any edit request from the GhostText extension (sent over a [WebSocket](https://developer.mozilla.org/en-US/docs/Web/API/WebSockets_API)) as a file and opening said file with your preferred `$EDITOR`. Whenever the file is written to or your `$EDITOR` closes, the contents are sent back to the browser. Whenever the textbox is updated in your browser, the file is updated with the new text.

Want to reply to the comments on any website with [`ed`](https://www.gnu.org/fun/jokes/ed-msg.html)? Go right ahead.

## Getting Started

To use it:
1. [Install the browser extension](https://github.com/GhostText/GhostText#installation)
2. Install this: `cargo install ghosttext-any` (requires [cargo/rust](https://www.rust-lang.org/tools/install))
3. Run `gtany` in a terminal.
4. Click on a textbox in your browser and trigger the GhostText extension.
5. Tada! Your `$EDITOR` is opened in the same terminal with the content of the textbox. Write, quit, and the same content will be updated in your browser.

By default, `gtany` only spawns a single instance at a time (based on the assumption that your `$EDITOR` uses the terminal it's spawned in, and you don't want multiple instances fighting over `/dev/tty`). If you'd like multiple concurrent instances to be spawned, use the `-m`/`--multi` flag.

If you don't have `$EDITOR` set or you'd like to run something else, you can specify a command to run with the `-e`/`--editor` flag.

For example, if you'd like to spawn a new terminal window with your `$EDITOR` whenever you use GhostText, you could use a command like this:
```shell
gtany --multi --editor "x-terminal-emulator -e $EDITOR"
```
(If you don't use a Unix-y OS or do but not with [X11](https://en.wikipedia.org/wiki/X_Window_System) or do but not with a terminal emulator that supports `-e`, you'll need to figure something else out).

## Systemd Socket Activation

If you use a Linux distribution with systemd, you can run GhostText-Any as a socket-activated service, where systemd watches the GhostText port and _only starts GhostText-Any when you use the browser extension_. Combined with the `--idle-timeout` flag, it will automatically start up and shut down when the browser extension is closed.

1. Build GhostText-Any with systemd support enabled: `cargo install ghosttext-any --features systemd`
2. Copy the example systemd files from this repo, [`gtany.socket`](contrib/gtany.socket) and [`gtany.service`](contrib/gtany.service), to `~/.config/systemd/user/`
3. Update the `ExecStart` field in `gtany.service` to call your preferred `$EDITOR`.
4. Load the units: `systemctl --user daemon-reload`
5. Enable the socket: `systemctl --user enable gtany.socket`
6. Check the status: `systemctl --user status gtany.{socket,service}`
