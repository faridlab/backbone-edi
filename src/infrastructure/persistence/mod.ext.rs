// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The hand-written EDI SQL's parameter/projection types (see `edi_document_repository` and
// `trading_partner_repository`, both declared `user_owned` in metaphor.codegen.yaml).
pub use edi_document_repository::{AckRow, DocOutcomeRow, NewInboundDocRow};
pub use trading_partner_repository::NewPartnerRow;
