# game

Rust workspace: an authoritative engine plus a dependency-free wire protocol.
Any display (three.js now, a Rust renderer later) just speaks the protocol.

    cargo test            # protocol + world rules
    cargo run -p engine   # ws://127.0.0.1:3000/ws  (PORT env var to change)

Protocol details live at the top of `protocol/src/lib.rs`.
World space: units are tiles, +x right, +y UP (flip y in the client if needed).
