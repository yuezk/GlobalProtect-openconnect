<p align="center">
  <img src="https://github.com/yuezk/GlobalProtect-openconnect/assets/3297602/9242df9c-217d-42ab-8c21-8f9f69cd4eb5">
</p>

## Development

### Dependencies

The following packages will be required to build depending on your environment:

- Cargo
- openconnect-devel (containing `openconnect.h`)
- pnpm

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
