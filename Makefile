.PHONY: install build test lint fix clean coverage tap-push install-hooks install-pre-commit-hook

install:
	cargo install --path .

build:
	cargo build --release

test:
	cargo test

lint:
	cargo fmt --check && cargo clippy -- -D warnings

fix:
	cargo fmt && cargo clippy --fix --allow-dirty

clean:
	cargo clean

coverage:
	cargo llvm-cov --fail-under-lines 80 --fail-under-functions 80 --ignore-filename-regex '(main\.rs|config\.rs|binary\.rs|file_ref\.rs|signal\.rs|supervisor\.rs|watcher\.rs)'

tap-push:
	@which shasum > /dev/null 2>&1 || { echo "shasum not found"; exit 1; }
	@which gh > /dev/null 2>&1 || { echo "gh CLI not found. Install: brew install gh"; exit 1; }
	@VERSION=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	test -n "$$VERSION" || { echo "FAIL: could not read version from Cargo.toml"; exit 1; }; \
	TAG="v$$VERSION"; \
	echo "folor $$VERSION"; \
	ASSET="folor-$${TAG}-aarch64-apple-darwin.tar.gz"; \
	test -d ../homebrew-tap/.git || { echo "Tap repo not found at ../homebrew-tap — clone it: git clone git@github.com:sdkks/homebrew-tap.git ../homebrew-tap"; exit 1; }; \
	rm -rf /tmp/folor-tap && mkdir -p /tmp/folor-tap; \
	echo "Downloading $$ASSET from GitHub release..."; \
	gh release download "$$TAG" \
		--repo sdkks/folor \
		--pattern "$$ASSET" \
		--dir /tmp/folor-tap; \
	test -f "/tmp/folor-tap/$$ASSET" || { echo "FAIL: download did not produce the archive"; exit 1; }; \
	SHA256=$$(shasum -a 256 "/tmp/folor-tap/$$ASSET" | awk '{print $$1}'); \
	test -n "$$SHA256" || { echo "FAIL: SHA256 computation returned empty"; exit 1; }; \
	echo "SHA256: $$SHA256"; \
	sed \
		-e "s/REPLACE_WITH_VERSION/$$VERSION/g" \
		-e "s/REPLACE_WITH_SHA256/$$SHA256/g" \
		tap/Formula/folor.rb > /tmp/folor-tap/folor.rb; \
	cd ../homebrew-tap && git pull --rebase; \
	cp /tmp/folor-tap/folor.rb ../homebrew-tap/Formula/folor.rb; \
	cd ../homebrew-tap && \
		git add Formula/folor.rb && \
		git diff --cached --quiet || git commit -m "folor $$VERSION"; \
	cd ../homebrew-tap && git push; \
	rm -rf /tmp/folor-tap; \
	echo "Pushed folor $$VERSION formula to homebrew-tap"

install-hooks: install-pre-commit-hook

install-pre-commit-hook:
	cp pre-commit.sh .git/hooks/pre-commit
	cp commit-msg.sh .git/hooks/commit-msg
	chmod +x .git/hooks/pre-commit .git/hooks/commit-msg
