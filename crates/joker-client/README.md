# joker-client

The Rust client of the local API of `jokerd` (ref/05 part B, ref/09): node targets (`local`, `unix:<path>`, `ssh://[user@]host[:port]`, contexts from `config.toml`), a blocking HTTP/1.1 client over the Unix socket, and the client obligations of ref/09 §9.3 (`Joker-Nonce`, `Joker-Timestamp`, `Idempotency-Key` on every mutating request).
