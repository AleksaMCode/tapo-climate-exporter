CARGO   := cargo
MANIFEST := exporter/Cargo.toml
COMPOSE := docker compose

# Coverage tool version, used by coverage-tools.
LLVM_COV_VERSION := 0.9.2

# Security audit tool version, used by audit-tools.
AUDIT_VERSION := 0.23.0

# Stops Git Bash on Windows from rewriting container paths such as /src.
export MSYS_NO_PATHCONV := 1

.DEFAULT_GOAL := help
.PHONY: help build run test coverage-tools coverage fmt fmt-check lint audit-tools audit up rebuild down logs

help: ## List the available commands
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F ':.*## ' '{printf "  make %-12s %s\n", $$1, $$2}'

build: ## Compile the exporter (release binary in exporter/target/release)
	$(CARGO) build --release --locked --manifest-path $(MANIFEST)

run: ## Compile if needed, then run the exporter on this machine (reads .env)
	$(CARGO) run --release --locked --manifest-path $(MANIFEST)

test: ## Run the exporter's unit tests
	$(CARGO) test --locked --manifest-path $(MANIFEST)

coverage-tools: ## Install cargo-llvm-cov locally (needed once for `make coverage`)
	rustup component add llvm-tools-preview
	$(CARGO) install cargo-llvm-cov --version $(LLVM_COV_VERSION) --locked

coverage: ## Run the unit tests with coverage and print the percentages
	$(CARGO) llvm-cov --locked --manifest-path $(MANIFEST)

fmt: ## Format the Rust code with rustfmt
	$(CARGO) fmt --manifest-path $(MANIFEST)

fmt-check: ## Check formatting without changing any file
	$(CARGO) fmt --manifest-path $(MANIFEST) --check

lint: ## Run the clippy linter; warnings count as errors
	$(CARGO) clippy --locked --all-targets --manifest-path $(MANIFEST) -- -D warnings

audit-tools: ## Install cargo-audit locally (needed once for `make audit`)
	$(CARGO) install cargo-audit --version $(AUDIT_VERSION) --locked

audit: ## Check all dependencies in Cargo.lock for known security advisories
	$(CARGO) audit --file exporter/Cargo.lock

up: ## Build and start everything in Docker: exporter, Prometheus, Grafana
	$(COMPOSE) up -d --build

rebuild: ## Rebuild the images without the layer cache, then recreate all containers
	$(COMPOSE) build --no-cache
	$(COMPOSE) up -d --force-recreate

down: ## Stop and remove the containers (history is kept in the volumes)
	$(COMPOSE) down

logs: ## Follow the logs of the running containers
	$(COMPOSE) logs -f --tail=50
