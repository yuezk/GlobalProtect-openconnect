#!/bin/bash

# Post-install message for GlobalProtect OpenConnect conda package
# This script displays setup instructions after installation

cat << 'EOF'

================================================================================
🎉 GlobalProtect OpenConnect has been successfully installed!
================================================================================

QUICK START:
------------

1. Run the setup script to configure your system:
   gp-setup --all

2. Test authentication with your VPN server:
   gpauth --browser default your-vpn-server.com

3. Connect to VPN:
   gpclient connect your-vpn-server.com

IMPORTANT SETUP NOTES:
----------------------

🔧 CONFIGURATION SCRIPT:
   Run 'gp-setup --all' to automatically configure:
   - URL scheme handler for browser authentication
   - Runtime directories and permissions
   - Flatpak browser permissions (if applicable)

🌐 BROWSER AUTHENTICATION:
   For proper browser-based SAML authentication, the system needs to register
   the 'globalprotectcallback://' URL scheme handler. The setup script does
   this automatically.

🔒 FILE PERMISSIONS:
   The applications automatically use appropriate directories:
   - Root users: /var/run/ (system-wide)
   - Regular users: ~/.local/state/globalprotect/ (user-specific)

📦 FLATPAK BROWSERS:
   If you use Flatpak browsers (Firefox, Chrome, etc.), additional permissions
   may be needed. The setup script configures these automatically.

MANUAL SETUP (if needed):
-------------------------

If you prefer manual configuration or encounter issues:

1. URL Scheme Handler:
   gp-setup --url-handler

2. Runtime Directories:
   gp-setup --runtime-dirs

3. File Permissions:
   gp-setup --permissions

4. Flatpak Configuration:
   gp-setup --flatpak

TROUBLESHOOTING:
----------------

• Check current configuration:
  gp-setup --check

• If authentication hangs in browser:
  - Ensure URL scheme handler is registered
  - Check Flatpak permissions for browsers
  - Try: gp-setup --url-handler

• Permission errors:
  - Run: gp-setup --permissions
  - Ensure proper runtime directory access

• For Flatpak permission issues:
  - Install Flatseal: flatpak install flathub com.github.tchx84.Flatseal
  - Grant Network and Filesystem permissions to your browser

DOCUMENTATION:
--------------

📚 Full documentation: https://github.com/yuezk/GlobalProtect-openconnect
📖 Operator's Guide: Available in the repository docs/ directory

SUPPORT:
--------

🐛 Issues: https://github.com/yuezk/GlobalProtect-openconnect/issues
💬 Discussions: https://github.com/yuezk/GlobalProtect-openconnect/discussions

================================================================================

Next step: Run 'gp-setup --all' to configure your system automatically!

EOF
