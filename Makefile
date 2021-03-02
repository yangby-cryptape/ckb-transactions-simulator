BIN := target/release/ckb-transactions-simulator
JSONRPC_URL := http://127.0.0.1:8114

${BIN}:
	@RUST_LOG=info,ckb_transactions_simulator=trace \
		cargo build --release

build: delete-bin ${BIN}

delete-bin:
	@rm -f "${BIN}"

test: ${BIN}
	"${BIN}"      --version
	"${BIN}" init --version
	"${BIN}" run  --version

clean:
	@rm -rf ${BIN} data/

init: ${BIN}
	@RUST_LOG=info,ckb_transactions_simulator=trace \
		"${BIN}" init \
			--data-dir data \
			--config configs/init.yaml

run: ${BIN}
	@RUST_LOG=info,ckb_transactions_simulator=trace \
		"${BIN}" run \
			--data-dir data \
			--jsonrpc-url "${JSONRPC_URL}" \
			--config configs/run.yaml
