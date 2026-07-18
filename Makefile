.PHONY: test test-assets

test:
	./scripts/test.sh

test-assets:
	./scripts/validate_assets.sh
