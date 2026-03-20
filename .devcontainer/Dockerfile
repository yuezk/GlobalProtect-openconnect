FROM ubuntu:22.04

ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=$USER_UID

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    RUST_VERSION=1.85.0 \
    DEBIAN_FRONTEND=noninteractive

# Install all system dependencies in a single layer and clean up apt cache
RUN set -eux; \
  apt-get update; \
  apt-get install -y --no-install-recommends \
    # Common utilities
    dh-autoreconf \
    sudo \
    ca-certificates \
    curl \
    gnupg \
    git \
    less \
    wget \
    file \
    software-properties-common \
    # libopenconnect-dev to install the related dependencies for building openconnect from source
    # Otherwise, we have to install them manually one by one
    libopenconnect-dev \
    # Tauri dependencies
    libwebkit2gtk-4.1-dev \
    build-essential \
    libxdo-dev \
    libssl-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev; \
  # Clean up apt cache to reduce image size
  apt-get clean; \
  rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN set -eux; \
  groupadd --gid $USER_GID $USERNAME; \
  useradd --uid $USER_UID --gid $USER_GID -m $USERNAME; \
  echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME; \
  chmod 0440 /etc/sudoers.d/$USERNAME

# Install Rust
RUN set -eux; \
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain $RUST_VERSION --profile minimal; \
  chown -R $USERNAME:$USERNAME $RUSTUP_HOME $CARGO_HOME; \
  rustup --version; \
  cargo --version; \
  rustc --version

# Install Node.js and global npm packages
RUN set -eux; \
  curl -fsSL https://deb.nodesource.com/setup_20.x | bash -; \
  apt-get install -y --no-install-recommends nodejs; \
  corepack enable; \
  npm install -g diff-so-fancy; \
  # Clean up apt cache
  apt-get clean; \
  rm -rf /var/lib/apt/lists/*

USER $USERNAME

# Install Oh My Zsh
RUN sh -c "$(wget -O- https://github.com/deluan/zsh-in-docker/releases/download/v1.1.5/zsh-in-docker.sh)" -- \
    -t https://github.com/denysdovhan/spaceship-prompt \
    -a 'SPACESHIP_PROMPT_ADD_NEWLINE="false"' \
    -a 'SPACESHIP_PROMPT_SEPARATE_LINE="false"' \
    -p git \
    -p https://github.com/zsh-users/zsh-autosuggestions \
    -p https://github.com/zsh-users/zsh-completions; \
    # Change the default shell
    sudo chsh -s /bin/zsh $USERNAME; \
    # Change the XTERM to xterm-256color
    sed -i 's/TERM=xterm/TERM=xterm-256color/g' $HOME/.zshrc;
