//! Shared test helpers: a live pool, a fake mapper (records / can reject), a REAL backbone-selling mapper
//! (maps an inbound PO to a genuine sales order), and a capturing event sink.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use backbone_edi::application::service::edi_events::{EdiEvent, EdiEventSink};
use backbone_edi::application::service::edi_ports::{MapAck, MapRejected, MapRequest, MappingPort};
use sqlx::PgPool;
use uuid::Uuid;

pub fn dburl() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/backbone_edi".into())
}
pub async fn pool() -> PgPool {
    PgPool::connect(&dburl()).await.expect("connect")
}

/// A fake mapping target. Records every map; returns a synthetic internal ref, or rejects when armed.
#[derive(Clone, Default)]
pub struct FakeMapper {
    pub maps: Arc<Mutex<Vec<MapRequest>>>,
    pub reject: Arc<Mutex<Option<(String, String)>>>,
}
impl FakeMapper {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn rejecting(code: &str, message: &str) -> Self {
        let f = Self::default();
        *f.reject.lock().unwrap() = Some((code.into(), message.into()));
        f
    }
    pub fn count(&self) -> usize {
        self.maps.lock().unwrap().len()
    }
}
#[async_trait::async_trait]
impl MappingPort for FakeMapper {
    async fn map(&self, req: &MapRequest) -> Result<MapAck, MapRejected> {
        self.maps.lock().unwrap().push(req.clone());
        if let Some((code, message)) = self.reject.lock().unwrap().clone() {
            return Err(MapRejected { code, message });
        }
        Ok(MapAck { internal_ref_type: "sales_order".into(), internal_ref_id: Uuid::new_v4() })
    }
}

/// The ACL over the REAL backbone-selling module: maps an inbound PO payload to a genuine sales order.
pub struct RealSellingMapper {
    pub selling: backbone_selling::application::service::selling_write_service::SellingWriteService,
}
impl RealSellingMapper {
    pub fn new(pool: PgPool) -> Self {
        Self {
            selling: backbone_selling::application::service::selling_write_service::SellingWriteService::new(pool),
        }
    }
}
#[async_trait::async_trait]
impl MappingPort for RealSellingMapper {
    async fn map(&self, req: &MapRequest) -> Result<MapAck, MapRejected> {
        use backbone_selling::application::service::selling_write_service::{NewLine, NewSalesOrder};
        use rust_decimal::Decimal;
        let p = &req.payload;
        let customer_id: Uuid = p.get("customer_id").and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
            .ok_or(MapRejected { code: "bad_payload".into(), message: "missing customer_id".into() })?;
        let lines: Vec<NewLine> = p.get("lines").and_then(|v| v.as_array()).map(|arr| arr.iter().map(|l| NewLine {
            item_id: l.get("item_id").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or_else(Uuid::new_v4),
            revenue_account_id: None,
            description: None,
            quantity: l.get("qty").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(Decimal::ONE),
            unit_price: l.get("price").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(Decimal::ZERO),
            line_discount: Decimal::ZERO,
        }).collect()).unwrap_or_default();
        let order = NewSalesOrder {
            order_number: format!("EDI-{}", req.control_number),
            quotation_id: None, company_id: req.company_id, branch_id: None, customer_id,
            order_date: chrono::Utc::now().date_naive(), delivery_date: None, currency: None,
            tax_rate: Decimal::ZERO, notes: Some("Created from inbound EDI PO".into()), lines,
        };
        match self.selling.create_sales_order(order).await {
            Ok(id) => Ok(MapAck { internal_ref_type: "sales_order".into(), internal_ref_id: id }),
            Err(e) => Err(MapRejected { code: "selling_rejected".into(), message: e.to_string() }),
        }
    }
}

/// Captures EDI events.
#[derive(Clone, Default)]
pub struct CapturingSink {
    pub events: Arc<Mutex<Vec<EdiEvent>>>,
}
impl CapturingSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn mapped(&self) -> usize {
        self.events.lock().unwrap().iter().filter(|e| matches!(e, EdiEvent::EdiDocumentMapped(_))).count()
    }
    pub fn failed(&self) -> usize {
        self.events.lock().unwrap().iter().filter(|e| matches!(e, EdiEvent::EdiDocumentFailed { .. })).count()
    }
    pub fn acknowledged(&self) -> usize {
        self.events.lock().unwrap().iter().filter(|e| matches!(e, EdiEvent::EdiDocumentAcknowledged { .. })).count()
    }
}
impl EdiEventSink for CapturingSink {
    fn publish(&self, event: &EdiEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

/// A sink that drops every event — models a crash/loss between the DB commit and the in-proc publish.
pub struct DroppingSink;
impl EdiEventSink for DroppingSink {
    fn publish(&self, _e: &EdiEvent) {}
}
