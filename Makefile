ROOT := $(CURDIR)
APP_DIR := $(ROOT)/app
SECRETS_FILE := $(ROOT)/.secrets
WRANGLER_CONFIG := $(APP_DIR)/wrangler.toml
WRANGLER ?= npx wrangler
DEPLOY_ENV ?= CI=1

ONNX_BUNDLES_DIR := $(ROOT)/vendor/wasm/encodec-rs/bundles
ONNX_BUCKET := vin-yl-player-assets
ONNX_BUNDLE_ASSET_FILES := decode_frame.onnx lm_weights_q8.bin

.PHONY: build test deploy sync-onnx-assets

test:
	cargo test

build:
	cd "$(APP_DIR)" && npm run build

# Uploads the large EnCodec model weights (31-36MB each, over the 25MB
# Workers Assets limit) straight to R2, where the worker streams them from
# at request time. Only needs re-running when vendor/wasm/encodec-rs/bundles
# changes, not on every deploy.
sync-onnx-assets:
	@test -f "$(SECRETS_FILE)" || { echo "missing $(SECRETS_FILE)"; exit 1; }
	@set -a; . "$(SECRETS_FILE)"; set +a; \
	test -n "$${CLOUDFLARE_EMAIL:-}" || { echo "missing CLOUDFLARE_EMAIL"; exit 1; }; \
	test -n "$${CLOUDFLARE_API_KEY:-}" || { echo "missing CLOUDFLARE_API_KEY"; exit 1; }; \
	for bundle_dir in "$(ONNX_BUNDLES_DIR)"/*/; do \
		bundle_name=$$(basename "$$bundle_dir"); \
		for file in $(ONNX_BUNDLE_ASSET_FILES); do \
			echo "uploading $$bundle_name/$$file"; \
			CLOUDFLARE_EMAIL="$${CLOUDFLARE_EMAIL}" CLOUDFLARE_API_KEY="$${CLOUDFLARE_API_KEY}" \
				$(WRANGLER) r2 object put "$(ONNX_BUCKET)/$$bundle_name/$$file" \
				--file "$$bundle_dir$$file" --remote; \
		done; \
	done

deploy: test build
	@test -f "$(SECRETS_FILE)" || { echo "missing $(SECRETS_FILE)"; exit 1; }
	@set -a; . "$(SECRETS_FILE)"; set +a; \
	test -n "$${CLOUDFLARE_EMAIL:-}" || { echo "missing CLOUDFLARE_EMAIL"; exit 1; }; \
	test -n "$${CLOUDFLARE_API_KEY:-}" || { echo "missing CLOUDFLARE_API_KEY"; exit 1; }; \
	cd "$(APP_DIR)" && CLOUDFLARE_EMAIL="$${CLOUDFLARE_EMAIL}" CLOUDFLARE_API_KEY="$${CLOUDFLARE_API_KEY}" $(DEPLOY_ENV) $(WRANGLER) deploy --config "$(WRANGLER_CONFIG)"
