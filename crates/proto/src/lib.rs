//! nitya's gRPC API (ADR-0011).

pub mod v1 {
    tonic::include_proto!("nitya.v1");
}
