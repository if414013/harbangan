FROM node:20-bookworm-slim AS builder

WORKDIR /app

COPY package.json package-lock.json* ./
RUN npm ci --ignore-scripts

COPY tsconfig.json ./
COPY src/ src/
RUN npm run build

FROM node:20-bookworm-slim

# Install git, gh CLI, curl, jq
RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    curl \
    jq \
    ca-certificates \
    openssh-client \
    gnupg \
    && curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
       | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
       | tee /etc/apt/sources.list.d/github-cli.list > /dev/null \
    && apt-get update && apt-get install -y gh \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

# Install Rust toolchain (for running cargo clippy/test quality gates)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /app

COPY --from=builder /app/dist dist/
COPY --from=builder /app/node_modules node_modules/
COPY package.json ./

# Create workspace and data directories
RUN mkdir -p /workspace /data

ENV NODE_ENV=production
ENV WORKSPACE_DIR=/workspace
ENV DATA_DIR=/data

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=10s --retries=3 \
  CMD curl -fs http://localhost:3000/api/v1/health || exit 1

CMD ["node", "dist/index.js"]
