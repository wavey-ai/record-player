ROOT := $(CURDIR)
APP_DIR := $(ROOT)/app
SECRETS_FILE := $(ROOT)/.secrets
SECRETS_FILE_FALLBACK := $(ROOT)/../vin.yl.player/.secrets
WRANGLER_CONFIG := $(APP_DIR)/wrangler.toml
WRANGLER_BOOTSTRAP_DIR := $(ROOT)/tmp-wrangler
WRANGLER_NODE_BIN ?= $(HOME)/.nvm/versions/node/v22.22.3/bin
WRANGLER_NPM := PATH="$(WRANGLER_NODE_BIN):$$PATH" npm
WRANGLER ?= $(WRANGLER_BOOTSTRAP_DIR)/node_modules/.bin/wrangler
DEPLOY_ENV ?= CI=1

.PHONY: build test deploy wrangler-bootstrap

test:
	cargo test

build:
	cd "$(APP_DIR)" && npm run build

wrangler-bootstrap:
	@mkdir -p "$(WRANGLER_BOOTSTRAP_DIR)"
	@test -x "$(WRANGLER)" || { \
		cd "$(WRANGLER_BOOTSTRAP_DIR)" && $(WRANGLER_NPM) init -y >/dev/null 2>&1 && $(WRANGLER_NPM) install --ignore-scripts wrangler@4.110.0; \
	}

deploy: wrangler-bootstrap
	@secret_file="$(SECRETS_FILE)"; \
	if [ ! -f "$$secret_file" ] && [ -f "$(SECRETS_FILE_FALLBACK)" ]; then secret_file="$(SECRETS_FILE_FALLBACK)"; fi; \
	test -f "$$secret_file" || { echo "missing $$secret_file"; exit 1; }; \
	set -a; . "$$secret_file"; set +a; \
	test -n "$${CLOUDFLARE_EMAIL:-}" || { echo "missing CLOUDFLARE_EMAIL"; exit 1; }; \
	test -n "$${CLOUDFLARE_API_KEY:-}" || { echo "missing CLOUDFLARE_API_KEY"; exit 1; }; \
	cd "$(APP_DIR)" && CLOUDFLARE_EMAIL="$${CLOUDFLARE_EMAIL}" CLOUDFLARE_API_KEY="$${CLOUDFLARE_API_KEY}" $(DEPLOY_ENV) npm run build && CLOUDFLARE_EMAIL="$${CLOUDFLARE_EMAIL}" CLOUDFLARE_API_KEY="$${CLOUDFLARE_API_KEY}" $(DEPLOY_ENV) $(WRANGLER) deploy --config "$(WRANGLER_CONFIG)"
