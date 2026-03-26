pub use orchestrator_client::{MAX_GRPC_DECODE_SIZE, TransportKind, connect};

/// Return the max decoding size for clients created from our channel.
pub fn max_decode_size() -> usize {
    MAX_GRPC_DECODE_SIZE
}
