# GlobalProtect OpenConnect - Conda Package

This conda package provides a complete installation of GlobalProtect OpenConnect, including automated system configuration tools.

## Quick Start

1. **Install the package:**
   ```bash
   conda install -c conda-forge globalprotect-openconnect
   ```

2. **Run the setup script:**
   ```bash
   gp-setup --all
   ```

3. **Test authentication:**
   ```bash
   gpauth --browser default your-vpn-server.com
   ```

4. **Connect to VPN:**
   ```bash
   gpclient connect your-vpn-server.com
   ```

## What's Included

This conda package includes:

- **CLI Applications:**
  - `gpclient` - Main VPN client
  - `gpservice` - Background service
  - `gpauth` - Authentication handler
  - `gpgui-helper` - GUI helper utilities
  - `gp-setup` - System configuration script

- **Desktop Integration:**
  - Application menu entries
  - Icon files
  - URL scheme handlers

- **Configuration Tools:**
  - Automated setup script
  - Permission management
  - Runtime directory configuration

## Setup Script (`gp-setup`)

The `gp-setup` script automates system configuration based on best practices and fixes common issues:

### Usage

```bash
gp-setup [OPTIONS]

OPTIONS:
    --user                Setup for current user only (default)
    --system              Setup system-wide configuration (requires root)
    --url-handler         Setup URL scheme handler for browser authentication
    --runtime-dirs        Create and configure runtime directories
    --permissions         Fix file permissions
    --flatpak             Configure Flatpak browser permissions
    --all                 Run all setup steps
    --check               Check current configuration
    --uninstall           Remove configuration files
    --help                Show help message
```

### Key Features

#### 🌐 URL Scheme Handler Setup
Configures the `globalprotectcallback://` URL scheme for proper browser authentication:
- Creates desktop file for URL handling
- Registers with system MIME database
- Verifies registration

#### 🔒 Runtime Directory Management
Sets up appropriate runtime directories based on user privileges:
- **Root users:** Uses `/var/run/` for system-wide operation
- **Regular users:** Uses `~/.local/state/globalprotect/` for user-specific operation
- Automatic permission management

#### 📦 Flatpak Browser Support
Configures permissions for Flatpak browsers:
- Detects installed Flatpak browsers
- Grants necessary network and filesystem permissions
- Provides Flatseal configuration guidance

#### 🛠️ Permission Management
Ensures proper file permissions:
- Runtime directory permissions
- Configuration file permissions
- Lock file cleanup

### Examples

```bash
# Complete automated setup
gp-setup --all

# User-only setup with URL handler
gp-setup --user --url-handler

# Check current configuration
gp-setup --check

# System-wide setup (requires root)
sudo gp-setup --system

# Configure only Flatpak browsers
gp-setup --flatpak

# Remove all configuration
gp-setup --uninstall
```

## Configuration

### Automatic Configuration

The applications automatically handle:

- **Runtime directory selection** based on user privileges
- **Permission validation** before file operations
- **Multi-user isolation** for shared systems
- **Error messaging** with actionable suggestions

### Manual Configuration

If needed, you can manually configure:

#### URL Scheme Handler
```bash
# Create desktop file
mkdir -p ~/.local/share/applications
cat > ~/.local/share/applications/gpclient-callback.desktop << 'EOF'
[Desktop Entry]
Type=Application
Name=GlobalProtect Callback Handler
Exec=gpclient launch-gui %u
MimeType=x-scheme-handler/globalprotectcallback;
NoDisplay=true
Categories=Network;
EOF

# Register scheme
update-desktop-database ~/.local/share/applications/
xdg-mime default gpclient-callback.desktop x-scheme-handler/globalprotectcallback
```

#### Runtime Directories
```bash
# For regular users
mkdir -p ~/.local/state/globalprotect
chmod 700 ~/.local/state/globalprotect

# For system deployment
sudo mkdir -p /var/run
```

## Troubleshooting

### Common Issues

#### Browser Authentication Hangs
**Symptom:** Authentication completes in browser but `gpauth` doesn't return output

**Solution:**
```bash
# Run URL handler setup
gp-setup --url-handler

# Verify registration
gp-setup --check
```

#### Permission Errors
**Symptom:** "Cannot access lock file" or similar permission errors

**Solution:**
```bash
# Fix permissions
gp-setup --permissions

# Check configuration
gp-setup --check
```

#### Flatpak Browser Issues
**Symptom:** Browser authentication fails with Flatpak browsers

**Solution:**
```bash
# Configure Flatpak permissions
gp-setup --flatpak

# Install Flatseal for advanced permission management
flatpak install flathub com.github.tchx84.Flatseal
```

### Diagnostic Commands

```bash
# Check overall configuration
gp-setup --check

# Test URL scheme registration
xdg-mime query default x-scheme-handler/globalprotectcallback

# Check runtime directory permissions
ls -la ~/.local/state/globalprotect/
ls -la $XDG_RUNTIME_DIR/gp*

# Test authentication with verbose logging
gpauth --verbose --browser default your-server.com
```

## Security Features

### Automatic Privilege Detection
- Uses system directories (`/var/run/`) when running as root
- Uses user directories (`~/.local/state/`) for regular users
- Prevents privilege escalation issues

### Multi-User Isolation
- Each user has separate runtime directories
- No interference between user sessions
- Automatic cleanup on user logout (XDG_RUNTIME_DIR)

### Permission Validation
- Checks file access before operations
- Creates directories with proper permissions
- Provides clear error messages with solutions

## Environment Variables

Optional environment variables for customization:

```bash
# Browser selection
export GP_BROWSER="firefox"
export GP_BROWSER="flatpak run org.mozilla.firefox"

# Logging
export GP_LOG_LEVEL="debug"

# Custom runtime directory (advanced)
export XDG_RUNTIME_DIR="/custom/runtime/dir"
```

## Integration with Other Package Managers

This conda package can coexist with installations from other package managers:

- **Native packages** (rpm, deb): May conflict with system files
- **Flatpak**: Requires additional permission setup
- **AppImage**: Works independently
- **Manual builds**: Use different installation prefixes

## Support

### Documentation
- [Main Repository](https://github.com/yuezk/GlobalProtect-openconnect)
- [Operator's Guide](https://github.com/yuezk/GlobalProtect-openconnect/blob/main/docs/operators-guide.adoc)
- [User Manual](https://github.com/yuezk/GlobalProtect-openconnect/blob/main/README.md)

### Getting Help
- [Issues](https://github.com/yuezk/GlobalProtect-openconnect/issues)
- [Discussions](https://github.com/yuezk/GlobalProtect-openconnect/discussions)
- [Conda-Forge Feedstock](https://github.com/conda-forge/globalprotect-openconnect-feedstock)

### Contributing
- Report issues with conda package: Include `gp-setup --check` output
- Request features: Use GitHub discussions
- Contribute fixes: Submit pull requests to main repository

## License

GPL-3.0 - See [LICENSE](https://github.com/yuezk/GlobalProtect-openconnect/blob/main/LICENSE) file for details.

## Changelog

See the main repository for detailed changelog information.