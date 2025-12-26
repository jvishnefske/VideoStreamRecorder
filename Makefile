.PHONY: build test coverage clean

build:
	cargo build --release

test:
	cargo test

coverage:
	cargo tarpaulin --out Html --out Xml --output-dir coverage

clean:
	cargo clean
	rm -rf coverage
