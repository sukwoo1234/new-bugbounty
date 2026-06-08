TARGET ?= onnx
SECONDS ?= 10
BACKEND ?= local-harness
BACKENDS ?=
HOURS ?= 168
ID ?= campaign-$(TARGET)-$(shell date +%Y%m%d-%H%M%S)

.PHONY: build preflight smoke smoke-all campaign-parallel campaign-serial

build:
	cargo build --offline

preflight:
	bash scripts/fuzz_host_preflight.sh --target $(TARGET)

smoke:
	bash scripts/fuzz_smoke.sh --target $(TARGET) --backend $(BACKEND) --seconds $(SECONDS)

smoke-all:
	bash scripts/fuzz_smoke.sh --target $(TARGET) --all --seconds $(SECONDS)

campaign-parallel:
	target/debug/tool campaign --mode parallel --target $(TARGET) --hours $(HOURS) --campaign-id $(ID) $(if $(BACKENDS),--backends $(BACKENDS),)

campaign-serial:
	target/debug/tool campaign --mode serial --target $(TARGET) --hours $(HOURS) --campaign-id $(ID) $(if $(BACKENDS),--backends $(BACKENDS),)
