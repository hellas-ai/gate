# Gate GUI with Embedded Daemon

This is the Tauri desktop application for Gate that includes an embedded daemon server.

## Architecture

The GUI app runs the Gate daemon as an embedded library within the same process, rather than spawning it as a separate binary.

The GUI frontend (frontend-tauri) itself is a minimal widget that shows essential info (e.g boostrap token) and directs user to the localhost server (frontend-daemon).

## Development

To run the GUI app in development mode:

```bash
make gui-build-dev
./target/debug/gate_gui
```

To build for production:

```bash
make gui-build
./target/release/gate_gui
```
