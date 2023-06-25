## Development

### Build the service

```sh
# Build the client first
cargo build -p gpclient

# Build the service
cargo build -p gpservice
```

### Start the service

```sh
sudo ./target/debug/gpservice
```

### Start the GUI

```sh
cd gpgui
pnpm install
pnpm tauri dev
```

### Open the DevTools

Right-click on the GUI window and select "Inspect Element".
