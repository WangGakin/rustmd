//! Agent Client Protocol (ACP) integration for Writ.
//!
//! This module implements the client side of ACP, allowing Writ to communicate
//! with AI coding agents over JSON-RPC via stdio.

mod client;

pub use client::{AcpClient, AcpEvent};
