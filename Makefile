# chronicle-registry — SPEL Program
#
# Quick start:
#   make build idl deploy setup
#   make cli ARGS="<command> --arg1 value1"


SHELL := /bin/bash
STATE_FILE := .chronicle_registry-state
IDL_FILE := chronicle-registry-idl.json
PROGRAMS_DIR := methods/guest/target/riscv32im-risc0-zkvm-elf/docker
PROGRAM_BIN := $(PROGRAMS_DIR)/chronicle_registry.bin

# Load saved state if it exists
-include $(STATE_FILE)

define save_var
	@grep -v '^$(1)=' $(STATE_FILE) 2>/dev/null > $(STATE_FILE).tmp || true
	@echo '$(1)=$(2)' >> $(STATE_FILE).tmp
	@mv $(STATE_FILE).tmp $(STATE_FILE)
endef

FFI_VENDORED := logos-chronicle/vendored/libchronicle_registry_ffi.so
FFI_SRCS := $(shell find ffi/src chronicle_registry_core/src -name '*.rs' 2>/dev/null) ffi/Cargo.toml ffi/Cargo.lock chronicle_registry_core/Cargo.toml

.PHONY: help build idl cli deploy setup inspect status clean ffi clean-ffi

help: ## Show this help
	@echo "chronicle-registry — SPEL Program"
	@echo ""
	@echo "  make build       Build the guest binary (needs risc0 toolchain)"
	@echo "  make idl         Generate IDL from program source"
	@echo "  make cli ARGS=   Run the IDL-driven CLI (reads spel.toml for config)"
	@echo "  make deploy      Deploy program to sequencer"
	@echo "  make setup       Create accounts needed for the program"
	@echo "  make inspect     Show ProgramId for built binary"
	@echo "  make status      Show saved state and binary info"
	@echo "  make clean       Remove saved state"
	@echo ""
	@echo "Example:"
	@echo "  make build idl deploy"
	@echo "  make cli ARGS=\"--help\""
	@echo "  make cli ARGS=\"<command> --arg1 value1\""

build: ## Build the guest binary
	cargo risczero build --manifest-path methods/guest/Cargo.toml
	@echo ""
	@echo "✅ Guest binary built: $(PROGRAM_BIN)"
	@ls -la $(PROGRAM_BIN) 2>/dev/null || true

idl: ## Generate IDL JSON from program source
	cargo run --bin generate_idl > $(IDL_FILE)
	@echo "✅ IDL written to $(IDL_FILE)"

cli: ## Run the IDL-driven CLI (ARGS="...")
	cargo run --bin chronicle_registry_cli -- $(ARGS)

deploy: ## Deploy program to sequencer
	@test -f "$(PROGRAM_BIN)" || (echo "ERROR: Binary not found. Run 'make build' first."; exit 1)
	lgs wallet -- deploy-program $(PROGRAM_BIN)
	@echo "✅ Program deployed"

inspect: ## Show ProgramId for built binary
	cargo run --bin chronicle_registry_cli -- inspect $(PROGRAM_BIN)

setup: ## Create accounts needed for the program
	@echo "Creating signer account..."
	$(eval SIGNER_ID := $(shell lgs wallet -- account new public 2>&1 | sed -n 's/.*Public\/\([A-Za-z0-9]*\).*/\1/p'))
	@echo "Signer: $(SIGNER_ID)"
	$(call save_var,SIGNER_ID,$(SIGNER_ID))
	@echo ""
	@echo "✅ Account saved to $(STATE_FILE)"

status: ## Show saved state and binary info
	@echo "chronicle-registry Status"
	@echo "──────────────────────────────────────"
	@if [ -f "$(STATE_FILE)" ]; then cat $(STATE_FILE); else echo "(no state — run 'make setup')"; fi
	@echo ""
	@echo "Binaries:"
	@ls -la $(PROGRAM_BIN) 2>/dev/null || echo "  chronicle_registry.bin: NOT BUILT (run 'make build')"
	@echo ""
	@echo "IDL:"
	@ls -la $(IDL_FILE) 2>/dev/null || echo "  $(IDL_FILE): NOT GENERATED (run 'make idl')"

clean: ## Remove saved state
	rm -f $(STATE_FILE) $(STATE_FILE).tmp
	@echo "✅ State cleaned"

ffi: $(FFI_VENDORED) ## Build the FFI cdylib and vendor it into logos-chronicle/
$(FFI_VENDORED): $(FFI_SRCS)
	cd ffi && cargo build --release
	@mkdir -p $(dir $(FFI_VENDORED))
	cp ffi/target/release/libchronicle_registry_ffi.so $(FFI_VENDORED)
	@# Flakes only ingest git-tracked files. `add -fN` registers the
	@# (gitignored) .so as intent-to-add so nix can see it without
	@# actually staging the binary for commit.
	@git add -fN $(FFI_VENDORED) 2>/dev/null || true
	@echo "✅ Vendored FFI -> $(FFI_VENDORED)"
	@ls -lh $(FFI_VENDORED)

clean-ffi: ## Remove the vendored FFI and cargo target dir
	rm -rf ffi/target $(FFI_VENDORED)
	@echo "✅ FFI build artifacts cleaned"
